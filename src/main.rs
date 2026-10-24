use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;

mod cli;
mod commands;
mod editor;
use cli::*;

impl Cli {
    fn to_overrides(&self) -> atlassian_cli::CliOverrides {
        atlassian_cli::CliOverrides {
            domain: self.domain.clone(),
            email: self.email.clone(),
            token: self.token.clone(),
            client_id: self.client_id.clone(),
            client_secret: self.client_secret.clone(),
            cloud_id: self.cloud_id.clone(),
        }
    }
}

/// Restore the conventional Unix reaction to a closed pipe. Rust ignores
/// SIGPIPE by default, so `println!` panics with a broken-pipe error when a
/// consumer like `head` or `jq` exits early — fatal for a JSON-first CLI
/// whose stdout is routinely piped. SIG_DFL makes the process end quietly at
/// that point instead, exactly like `cat` or `grep`.
#[cfg(unix)]
fn reset_sigpipe() {
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn reset_sigpipe() {}

#[tokio::main]
async fn main() {
    reset_sigpipe();
    let cli = Cli::parse();

    let log_level = match cli.verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };

    tracing_subscriber::fmt()
        .with_env_filter(log_level)
        .with_writer(std::io::stderr)
        .init();

    if let Err(err) = run(cli).await {
        let (code, payload) = render_error(&err);
        eprintln!("{payload}");
        std::process::exit(code);
    }
}

async fn run(cli: Cli) -> Result<()> {
    let overrides = cli.to_overrides();
    let config_path = cli.config.clone();
    let profile = cli.profile.clone();

    match cli.command {
        Command::Config(cmd) => {
            handle_config(cmd, config_path.as_ref(), profile.as_ref(), overrides).await
        }
        Command::Auth(cmd) => handle_auth(cmd, config_path, profile, overrides).await,
        Command::Jira(cmd) => {
            let config =
                atlassian_cli::Config::load(config_path.as_ref(), profile.as_ref(), overrides)?;

            let client = atlassian_cli::ApiClient::new(config).await?;
            let result = commands::jira::run(cmd, &client).await?;
            output_json(&result, cli.pretty);
            Ok(())
        }
        Command::Confluence(cmd) => {
            let config =
                atlassian_cli::Config::load(config_path.as_ref(), profile.as_ref(), overrides)?;

            let client = atlassian_cli::ApiClient::new(config).await?;
            let result = commands::confluence::run(cmd, &client).await?;
            output_json(&result, cli.pretty);
            Ok(())
        }
        Command::Selfcmd(cmd) => {
            let result = handle_self(cmd).await?;
            output_json(&result, cli.pretty);
            Ok(())
        }
        Command::Completions { shell } => {
            use clap::CommandFactory;
            clap_complete::generate(
                shell,
                &mut Cli::command(),
                "atlassian-cli",
                &mut std::io::stdout(),
            );
            Ok(())
        }
    }
}

/// Render a failed run as a single-line JSON object on stderr and pick the
/// exit code. Stdout stays reserved for results, so `| jq` pipelines see
/// either valid output or nothing.
///
/// Exit codes, stable for scripted callers: 1 generic, 2 CLI usage (clap),
/// 3 auth (401/403), 4 not found (404), 5 rate limited (429),
/// 6 server error (5xx). API failures carry `status`/`operation` (and
/// `hint` when remediation is known) alongside `message`.
async fn handle_self(cmd: SelfCommand) -> Result<serde_json::Value> {
    use atlassian_cli::dist;

    let installation = dist::Installation::detect()?;
    match cmd.subcommand {
        SelfSubcommand::Status => {
            self_status(
                &installation,
                atlassian_cli::auth::KeychainAccess::from_env(),
            )
            .await
        }
        SelfSubcommand::Update {
            version,
            force,
            verify_attestations,
        } => self_update(&installation, version, force, verify_attestations).await,
        SelfSubcommand::Skill { action } => match action {
            SelfSkillAction::Install => self_skill_install(&installation),
            SelfSkillAction::Remove { yes } => {
                if !yes {
                    anyhow::bail!("Removing the skill requires --yes");
                }
                self_skill_remove(&installation)
            }
        },
        SelfSubcommand::Uninstall {
            yes,
            keep_skill,
            keep_credentials,
            purge_config,
        } => {
            if !yes {
                anyhow::bail!("Uninstalling requires --yes");
            }
            self_uninstall(
                &installation,
                atlassian_cli::auth::KeychainAccess::from_env(),
                keep_skill,
                keep_credentials,
                purge_config,
            )
            .await
        }
    }
}

/// The skill directory, or the reason there is none to name.
fn skill_dir(installation: &atlassian_cli::dist::Installation) -> Result<std::path::PathBuf> {
    installation
        .skill_dir()
        .ok_or_else(|| anyhow::anyhow!("Failed to determine home directory"))
}

fn skill_report(dir: &std::path::Path) -> serde_json::Value {
    use atlassian_cli::dist::skill;
    serde_json::json!({
        "name": skill::SKILL_NAME,
        "path": display_path(dir),
        "state": skill::state(dir).as_str(),
        "version": skill::carried_version(),
    })
}

/// Everything this installation is, read from disk alone — no network, so the
/// command that answers "what have I got" never hangs on a release channel.
/// "Is there a newer one" is `self update`'s question, and it answers it
/// without changing anything when the running binary is already current.
async fn self_status(
    installation: &atlassian_cli::dist::Installation,
    keychain: atlassian_cli::auth::KeychainAccess,
) -> Result<serde_json::Value> {
    use atlassian_cli::dist;

    let credentials_file = installation.credentials_file();
    let stored = stored_profiles_of(installation, keychain).await;
    let config_file = installation.config_file();
    // Reported, not raised: this is the command a user runs after an uninstall
    // refused, and a path it cannot read is the answer they came for.
    let config_state = config_file
        .as_deref()
        .map(atlassian_cli::path_present)
        .transpose();

    Ok(serde_json::json!({
        "version": dist::current_version().to_string(),
        "target": dist::ReleaseTarget::current().map(|t| t.triple),
        "binary": display_path(installation.binary()),
        "skill": skill_dir(installation).ok().as_deref().map(skill_report),
        "config": {
            "path": config_file.as_deref().map(display_path),
            "exists": config_state.as_ref().ok().copied().flatten(),
            "error": config_state.as_ref().err().map(|e| format!("{e:#}")),
        },
        "credentials": credentials_report(&stored, credentials_file.as_deref()),
    }))
}

async fn self_update(
    installation: &atlassian_cli::dist::Installation,
    version: Option<String>,
    force: bool,
    verify_attestations: bool,
) -> Result<serde_json::Value> {
    use atlassian_cli::dist;

    let target = dist::ReleaseTarget::current()
        .ok_or_else(|| anyhow::anyhow!("{}", dist::ReleaseTarget::unsupported_reason()))?;
    let running = dist::current_version();
    let requested = version.as_deref().map(dist::parse_tag).transpose()?;

    let client = dist::ReleaseClient::github()?;
    // A named version is the answer; asking the channel for another one would
    // only introduce a way for the two to disagree.
    let latest = match requested {
        Some(_) => None,
        None => Some(client.resolve_latest().await?),
    };
    let provenance = latest.as_ref().map(|l| l.provenance.as_str());

    let decision = dist::decide(
        &running,
        requested.as_ref(),
        latest.as_ref().map(|l| &l.version),
        force,
    )
    .ok_or_else(|| anyhow::anyhow!("no version to install"))?;

    let to = match decision {
        dist::Decision::AlreadyCurrent(version) => {
            return Ok(serde_json::json!({
                "action": "already_current",
                "version": version.to_string(),
                "latestFrom": provenance,
            }));
        }
        dist::Decision::RefusedDowngrade { running, offered } => anyhow::bail!(
            "the latest release is {offered}, older than the running {running} — \
             install it deliberately with `--version {offered}` if that is intended"
        ),
        dist::Decision::Replace { to, .. } => to,
    };

    eprintln!("Downloading {}", target.archive_name(&to));
    let staging = dist::Staging::new()?;
    let binary =
        dist::fetch_verified_binary(&client, target, &to, &staging, verify_attestations).await?;
    dist::install(&staging.write(target.binary, &binary)?, &to)?;
    eprintln!("Replaced {}", installation.binary().display());

    Ok(serde_json::json!({
        "action": if running == to { "reinstalled" } else { "updated" },
        "from": running.to_string(),
        "to": to.to_string(),
        "target": target.triple,
        "binary": display_path(installation.binary()),
        "latestFrom": provenance,
        "attestationVerified": verify_attestations,
        "skill": redeploy_skill(installation),
    }))
}

/// Redeploy the skill through the binary that just landed.
///
/// This process carries the skill of the version being replaced, so writing it
/// from here would deploy the predecessor over the successor. Only where a
/// skill is already deployed: an installation that took none must not acquire
/// one from an update. Best-effort — the binary is in place and working, so a
/// skill that could not be written is a follow-up rather than a failed update.
fn redeploy_skill(installation: &atlassian_cli::dist::Installation) -> &'static str {
    use atlassian_cli::dist::skill::{self, SkillState};

    let Some(dir) = installation.skill_dir() else {
        return "skipped";
    };
    // Only `Absent` skips: an install that took no skill must not acquire one
    // from an update. Every other answer has something at that path, including
    // one that could not be read — and skipping on that is how the predecessor's
    // copy survives the binary that replaced it. A deploy that cannot repair it
    // reports its own failure below.
    if skill::state(&dir) == SkillState::Absent {
        return "absent";
    }
    let binary = installation.binary();
    // Probed, not read off the failure: a release from before `self` answers
    // the deploy with a usage error, and an operator told to run `self skill
    // install` on a binary that has no such subcommand gets the same error
    // again. Only naming an old `--version` reaches one of those, and there
    // the predecessor's skill is what stays.
    let carries_self = std::process::Command::new(binary)
        .args(["self", "--help"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !carries_self {
        eprintln!(
            "warning: the version now installed predates `self skill install`, so the deployed \
             skill was left as it is"
        );
        return "unsupported";
    }
    match std::process::Command::new(binary)
        .args(["self", "skill", "install"])
        .stdout(std::process::Stdio::null())
        .status()
    {
        Ok(status) if status.success() => "redeployed",
        // Not "stale": that is a byte comparison, and this is a deploy that
        // did not happen. The child names why on stderr.
        _ => {
            eprintln!(
                "warning: could not redeploy the skill — run `atlassian-cli self skill install`"
            );
            "failed"
        }
    }
}

fn self_skill_install(
    installation: &atlassian_cli::dist::Installation,
) -> Result<serde_json::Value> {
    use atlassian_cli::dist::skill;

    let dir = skill_dir(installation)?;
    let outcome = skill::deploy(&dir)?;
    Ok(serde_json::json!({
        "name": skill::SKILL_NAME,
        "path": display_path(&dir),
        "state": skill::state(&dir).as_str(),
        "version": skill::carried_version(),
        "written": outcome.written,
        "pruned": outcome.pruned,
    }))
}

fn self_skill_remove(
    installation: &atlassian_cli::dist::Installation,
) -> Result<serde_json::Value> {
    use atlassian_cli::dist::skill;

    let dir = skill_dir(installation)?;
    Ok(serde_json::json!({
        "name": skill::SKILL_NAME,
        "path": display_path(&dir),
        "removed": skill::remove(&dir)?,
    }))
}

/// Remove this installation, reporting each thing by name.
///
/// The order matters: the tokens go while there is still a binary that knows
/// where they are, and the binary goes last — on POSIX the running process
/// keeps its file open after the unlink. Project-level config
/// (`.atlassian.toml`) is never touched; it lives in the user's own repository.
async fn self_uninstall(
    installation: &atlassian_cli::dist::Installation,
    keychain: atlassian_cli::auth::KeychainAccess,
    keep_skill: bool,
    keep_credentials: bool,
    purge_config: bool,
) -> Result<serde_json::Value> {
    use atlassian_cli::dist::skill;

    // Before anything goes. Everything under the home directory is located
    // from it, so without one each step below would skip its target silently
    // and then take the binary that knows where it was — the same reason a
    // keychain that will not answer refuses here rather than proceeding. Only
    // what this run was actually asked to remove is at stake, and the refusal
    // names that rather than the whole set.
    let mut unlocatable: Vec<&str> = Vec::new();
    if !keep_skill && installation.skill_dir().is_none() {
        unlocatable.push("the deployed skill");
    }
    if !keep_credentials && installation.credentials_file().is_none() {
        unlocatable.push("the stored credentials");
    }
    if purge_config && installation.config_dir().is_none() {
        unlocatable.push("the global config");
    }
    if !unlocatable.is_empty() {
        anyhow::bail!(
            "Cannot locate the home directory, so {} cannot be found. Removing the binary \
             would leave that behind with nothing that knows where it lives — set HOME and \
             re-run, or leave it in place with --keep-skill / --keep-credentials / no \
             --purge-config.",
            unlocatable.join(" and ")
        );
    }

    let mut removed: Vec<serde_json::Value> = Vec::new();
    let mut kept: Vec<&str> = Vec::new();

    let credentials = if keep_credentials {
        kept.push("credentials");
        serde_json::Value::Null
    } else {
        // Every step from here can fail partway, and each one is irreversible,
        // so a failure has to say what it already did — "Permission denied"
        // alone reads as "nothing happened".
        clear_stored_tokens(installation, keychain, &mut removed)
            .await
            .map_err(|e| already_removed(e, &removed))?
    };

    if let Some(dir) = installation.skill_dir()
        && atlassian_cli::path_present(&dir).map_err(|e| already_removed(e, &removed))?
    {
        if keep_skill {
            kept.push("skill");
        } else if skill::remove(&dir).map_err(|e| already_removed(e.into(), &removed))? {
            record(&mut removed, "skill", display_path(&dir));
        }
    }

    // Remove the file this tool wrote, then the directory only if that leaves
    // it empty. `credentials.json` lives here too, so a whole-directory delete
    // would take it whatever `--keep-credentials` said — and would take
    // anything else the user keeps here besides.
    if let Some(dir) = installation.config_dir()
        && atlassian_cli::path_present(&dir).map_err(|e| already_removed(e, &removed))?
    {
        if purge_config {
            if let Some(file) = installation.config_file()
                && atlassian_cli::path_present(&file).map_err(|e| already_removed(e, &removed))?
            {
                std::fs::remove_file(&file).map_err(|e| already_removed(e.into(), &removed))?;
                record(&mut removed, "config", display_path(&file));
            }
            // Only when empty — anything else there is not this tool's. Saying
            // so keeps a directory that survived from going unmentioned. Not
            // emptied and not a plain directory are both definite answers
            // about one that stays; reading anything else as "kept" is how a
            // directory nobody could remove goes out as a clean uninstall.
            match std::fs::remove_dir(&dir) {
                Ok(()) => record(&mut removed, "config", display_path(&dir)),
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::DirectoryNotEmpty | std::io::ErrorKind::NotADirectory
                    ) =>
                {
                    kept.push("config-directory");
                }
                Err(e) => {
                    return Err(already_removed(
                        anyhow::Error::from(e)
                            .context(format!("Failed to remove {}", dir.display())),
                        &removed,
                    ));
                }
            }
        } else {
            kept.push("config");
        }
    }

    // Last, and through `self-replace`: Windows refuses to unlink a running
    // executable, so a plain `remove_file` would fail there after everything
    // above had already gone.
    let binary = installation.binary();
    if atlassian_cli::path_present(binary).map_err(|e| already_removed(e, &removed))? {
        if let Err(e) = self_replace::self_delete_at(binary) {
            return Err(already_removed(
                anyhow::Error::from(e).context(format!("Failed to remove {}", binary.display())),
                &removed,
            ));
        }
        record(&mut removed, "binary", display_path(binary));
    }

    Ok(serde_json::json!({
        "binary": display_path(binary),
        "removed": removed,
        "kept": kept,
        "credentials": credentials,
    }))
}

fn record(removed: &mut Vec<serde_json::Value>, kind: &str, target: String) {
    removed.push(serde_json::json!({ "kind": kind, "target": target }));
}

/// Attach what an uninstall already did to an error raised partway through.
///
/// Each step is irreversible and the record of them only exists here, so an
/// error that carries none leaves the user unable to tell whether their tokens
/// are gone.
fn already_removed(error: anyhow::Error, removed: &[serde_json::Value]) -> anyhow::Error {
    if removed.is_empty() {
        return error;
    }
    let done = removed
        .iter()
        .filter_map(|entry| entry["target"].as_str())
        .collect::<Vec<_>>()
        .join(", ");
    // Flattened rather than layered as context, so the failure reads first and
    // what it did not undo reads last.
    anyhow::anyhow!("{error:#} (already removed: {done})")
}

/// Discard every stored token, and the file that holds the fallback copies.
///
/// The file goes first and unconditionally: it belongs to this installation, so
/// a keychain that cannot be reached is no reason to leave it — and a corrupt
/// one still holds tokens whether or not its contents parsed.
///
/// The keychain is then enumerated rather than guessed at, each store in the
/// spelling its own search takes. Which answers refuse is the question of
/// whether a token could still be there afterwards:
///
/// - `Unsupported` — no store could be installed, so this binary never wrote a
///   token to one. Nothing to miss; proceed.
/// - `Skipped` — `ATLASSIAN_NO_KEYCHAIN` forbids the look, but a session from
///   before the flag was set may well be in there (see `src/auth/CLAUDE.md`).
/// - `Failed` — a store that exists and would not answer.
///
/// The last two refuse, because the step after this one removes the binary and
/// with it anything that knows where those tokens are.
async fn clear_stored_tokens(
    installation: &atlassian_cli::dist::Installation,
    keychain: atlassian_cli::auth::KeychainAccess,
    removed: &mut Vec<serde_json::Value>,
) -> Result<serde_json::Value> {
    // Enumerate before removing anything: the file is where the file-backed
    // profiles are named, so deleting it first would leave them out of the
    // report even though they were cleared.
    let stored = stored_profiles_of(installation, keychain).await;

    if let Some(file) = installation.credentials_file() {
        let path = file.clone();
        let cleared = tokio::task::spawn_blocking(move || {
            atlassian_cli::auth::remove_credentials_file(&path)
        })
        .await
        .context("Credentials file removal failed to complete")??;
        if cleared {
            record(removed, "credentials", display_path(&file));
        }
    }

    if let Some(refusal) = clear_stored_tokens_refusal(&stored.keyring) {
        anyhow::bail!(refusal);
    }

    for profile in &stored.profiles {
        token_store(installation, profile, keychain)?
            .delete()
            .await?;
        record(removed, "credentials", format!("profile:{profile}"));
    }

    Ok(credentials_report(
        &stored,
        installation.credentials_file().as_deref(),
    ))
}

/// Why the keychain half cannot be claimed, or `None` where it can.
///
/// Split out from the work so the decision can be exhausted without a machine's
/// keychain: it is the one that governs whether the binary comes off while a
/// token is still somewhere.
fn clear_stored_tokens_refusal(
    keyring: &atlassian_cli::auth::KeyringEnumeration,
) -> Option<String> {
    use atlassian_cli::auth::KeyringEnumeration;
    match keyring {
        KeyringEnumeration::Listed | KeyringEnumeration::Unsupported => None,
        KeyringEnumeration::Skipped => Some(
            "ATLASSIAN_NO_KEYCHAIN forbids reading the keychain, so any session stored there \
             before the flag was set cannot be cleared — unset it and re-run, or pass \
             --keep-credentials to leave stored tokens alone"
                .to_string(),
        ),
        KeyringEnumeration::Failed(reason) => Some(format!(
            "the keychain would not be listed ({reason}), so the tokens it holds cannot be \
             cleared — make it reachable and re-run, or pass --keep-credentials to finish \
             without touching what is stored there"
        )),
    }
}

/// The profiles this installation holds tokens for, read from its own paths
/// and the keychain it was told it may reach rather than the running
/// machine's.
async fn stored_profiles_of(
    installation: &atlassian_cli::dist::Installation,
    keychain: atlassian_cli::auth::KeychainAccess,
) -> atlassian_cli::auth::StoredProfiles {
    atlassian_cli::auth::stored_profiles(installation.credentials_file().as_deref(), keychain).await
}

/// A store over this installation's own backends rather than the running
/// machine's, so what an uninstall clears is what it enumerated.
fn token_store(
    installation: &atlassian_cli::dist::Installation,
    profile: &str,
    keychain: atlassian_cli::auth::KeychainAccess,
) -> Result<atlassian_cli::auth::TokenStore> {
    let file = installation
        .credentials_file()
        .ok_or_else(|| anyhow::anyhow!("Failed to determine home directory"))?;
    Ok(atlassian_cli::auth::TokenStore::at(profile, file, keychain))
}

/// Clear a profile's stored session and say what was there.
///
/// The read only names the backend for the message. It never decides whether
/// to clear: a read falls back to the file when the keychain will not answer,
/// so finding nothing there says nothing about what the keychain still holds —
/// and an entry that will not parse is still an entry to remove. `delete`
/// clears both backends and reports one that refused.
async fn clear_session(store: &atlassian_cli::auth::TokenStore, profile: &str) -> Result<()> {
    let found = store.load().await;
    store.delete().await?;
    match found {
        Ok(Some(loaded)) => println!(
            "✓ OAuth session cleared for profile '{profile}' ({})",
            loaded.backend
        ),
        Ok(None) => println!("No readable session for profile '{profile}'."),
        Err(e) => {
            println!("Cleared profile '{profile}'; what was stored could not be read ({e:#}).")
        }
    }
    if store.keychain() == atlassian_cli::auth::KeychainAccess::Forbidden {
        println!(
            "  ATLASSIAN_NO_KEYCHAIN is set, so the keychain was not touched. Unset it and \
             re-run to clear a session stored there before the flag."
        );
    }
    Ok(())
}

/// A path as JSON. `Path` serializes only when it is UTF-8 and `json!` panics
/// on the rest, which would answer with a crash instead of the single-line
/// error object every failure here is contracted to print.
fn display_path(path: &std::path::Path) -> String {
    path.display().to_string()
}

/// What the two token backends hold, and how completely each of them said so.
fn credentials_report(
    stored: &atlassian_cli::auth::StoredProfiles,
    file: Option<&std::path::Path>,
) -> serde_json::Value {
    serde_json::json!({
        "file": file.map(display_path),
        "fileError": stored.file_error,
        "profiles": stored.profiles,
        "keyring": keyring_report(&stored.keyring),
    })
}

/// How completely the keychain could be listed, with the reason only where
/// there is one.
fn keyring_report(outcome: &atlassian_cli::auth::KeyringEnumeration) -> serde_json::Value {
    match outcome.reason() {
        Some(reason) => serde_json::json!({ "enumeration": outcome.as_str(), "error": reason }),
        None => serde_json::json!({ "enumeration": outcome.as_str() }),
    }
}

fn render_error(err: &anyhow::Error) -> (i32, String) {
    let mut error = serde_json::json!({ "message": format!("{err:#}") });
    let mut code = 1;
    if let Some(api) = err.downcast_ref::<atlassian_cli::ApiError>() {
        error["status"] = api.status.as_u16().into();
        error["operation"] = api.operation.clone().into();
        if let Some(hint) = api.hint {
            error["hint"] = hint.into();
        }
        code = match api.status.as_u16() {
            401 | 403 => 3,
            404 => 4,
            429 => 5,
            s if s >= 500 => 6,
            _ => 1,
        };
    }
    (code, serde_json::json!({ "error": error }).to_string())
}

async fn handle_config(
    cmd: ConfigCommand,
    config_path: Option<&PathBuf>,
    profile: Option<&String>,
    overrides: atlassian_cli::CliOverrides,
) -> Result<()> {
    match cmd.subcommand {
        ConfigSubcommand::Init { global } => {
            let path = match config_path.filter(|_| !global) {
                Some(path) => atlassian_cli::Config::init_config_at(path)?,
                None => atlassian_cli::Config::init_config(global)?,
            };
            println!("Created config file: {:?}", path);
            println!("Edit it and add your credentials.");
            Ok(())
        }
        ConfigSubcommand::Show => {
            // Respect --config, --profile, and CLI overrides for accurate "resolved" view.
            let config =
                atlassian_cli::Config::load_without_validation(config_path, profile, overrides)?;
            print!("{}", config.redacted_toml()?);
            Ok(())
        }
        ConfigSubcommand::List => {
            println!("Configuration files (in precedence order):\n");

            let mut profiles: Vec<String> = Vec::new();
            // Raised, not warned: a file that will not parse still names
            // profiles, and printing the list without them says those profiles
            // do not exist. The header is already out by then — the exit code
            // is what says the listing is incomplete.
            let mut collect = |path: &std::path::Path| -> Result<()> {
                for name in atlassian_cli::Config::profile_names(path)? {
                    if !profiles.contains(&name) {
                        profiles.push(name);
                    }
                }
                Ok(())
            };

            if let Some(custom) = config_path {
                let there = atlassian_cli::path_present(custom)?;
                println!("Custom:  {:?} {}", custom, if there { "✓" } else { "✗" });
                collect(custom)?;
            }
            if let Some(project) = atlassian_cli::Config::project_config_path()? {
                println!("Project: {:?} ✓", project);
                collect(&project)?;
            } else {
                println!("Project: (none)");
            }
            if let Some(global) = atlassian_cli::Config::global_config_path() {
                let there = atlassian_cli::path_present(&global)?;
                println!("Global:  {:?} {}", global, if there { "✓" } else { "✗" });
                if there {
                    collect(&global)?;
                }
            }

            println!("\nProfiles (use --profile <name>):");
            for name in &profiles {
                println!("  {name}");
            }

            println!("\nEnvironment variables:");
            let env_vars = [
                ("ATLASSIAN_DOMAIN", false),
                ("ATLASSIAN_AUTH_METHOD", false),
                ("ATLASSIAN_EMAIL", false),
                ("ATLASSIAN_API_TOKEN", true),
                ("ATLASSIAN_CLIENT_ID", false),
                ("ATLASSIAN_CLIENT_SECRET", true),
                ("ATLASSIAN_CLOUD_ID", false),
            ];
            for (key, mask) in env_vars {
                let value = std::env::var(key)
                    .ok()
                    .map(|v| if mask { "***".to_string() } else { v });
                println!(
                    "  {}: {}",
                    key,
                    value.unwrap_or_else(|| "(not set)".to_string())
                );
            }

            Ok(())
        }
        ConfigSubcommand::Path { global } => {
            let path = atlassian_cli::Config::active_config_path(
                config_path.map(PathBuf::as_path),
                global,
            )?;
            println!("{}", path.display());
            Ok(())
        }
        ConfigSubcommand::Edit { global } => {
            let path = atlassian_cli::Config::active_config_path(
                config_path.map(PathBuf::as_path),
                global,
            )?;
            if !atlassian_cli::path_present(&path)? {
                anyhow::bail!(
                    "Config file does not exist: {:?}. Create it with config init using the same --config or --global selection.",
                    path
                );
            }

            let editor = match std::env::var("EDITOR") {
                Ok(editor) => editor,
                Err(std::env::VarError::NotPresent) => {
                    if cfg!(target_os = "macos") {
                        "open".to_string()
                    } else if cfg!(target_os = "windows") {
                        "notepad".to_string()
                    } else {
                        "vi".to_string()
                    }
                }
                Err(std::env::VarError::NotUnicode(_)) => {
                    anyhow::bail!("EDITOR must contain valid UTF-8");
                }
            };

            let status = editor::command(&editor, &path)?
                .status()
                .context("Failed to start editor")?;

            if !status.success() {
                anyhow::bail!("Failed to open editor");
            }

            println!("Config file edited: {:?}", path);
            Ok(())
        }
        ConfigSubcommand::Validate => {
            let config = atlassian_cli::Config::load(config_path, profile, overrides)?;

            // ApiClient::new() performs each strategy's credential check
            // (token fetch, cloud_id discovery, stored-token load). Any
            // failure here means credentials are invalid.
            let client = atlassian_cli::ApiClient::new(config).await?;
            let method = client.strategy().method();
            let identity = client.strategy().probe_identity(&client).await?;

            println!("✓ {} credentials valid", method);
            if let Some(domain) = client.config().domain.as_ref() {
                println!("  Domain: {}", domain);
            }
            if let Some(cid) = client.cloud_id() {
                println!("  Cloud ID: {}", cid);
            }
            match identity {
                Some(id) => {
                    println!("  User: {}", id.display_name);
                    if let Some(email) = id.email {
                        println!("  Email: {}", email);
                    }
                }
                None => {
                    // Non-probing principal (e.g. service_account) — credentials
                    // are already verified via the strategy's own check.
                    println!("  Identity: {}", client.strategy().identity_label());
                    println!(
                        "  Note: individual operations still require matching OAuth scopes and product permissions."
                    );
                }
            }
            Ok(())
        }
    }
}

fn output_json(value: &serde_json::Value, pretty: bool) {
    // Null is a sentinel used by streaming commands that have already
    // written to stdout — emitting "null" would corrupt that output.
    if value.is_null() {
        return;
    }
    if pretty {
        println!("{}", serde_json::to_string_pretty(value).unwrap());
    } else {
        println!("{}", serde_json::to_string(value).unwrap());
    }
}

async fn handle_auth(
    cmd: AuthCommand,
    config_path: Option<PathBuf>,
    profile: Option<String>,
    overrides: atlassian_cli::CliOverrides,
) -> Result<()> {
    use atlassian_cli::auth::{AuthMethod, OAuthStrategy, TokenStore};

    match cmd.subcommand {
        AuthSubcommand::Login { no_browser } => {
            // Validation-light load: the user is about to log in, so OAuth
            // tokens are absent and domain may be unset.
            let config = atlassian_cli::Config::load_without_validation(
                config_path.as_ref(),
                profile.as_ref(),
                overrides,
            )?;
            let params = config.oauth_params()?;
            let http = reqwest::Client::builder()
                .timeout(std::time::Duration::from_millis(
                    config.performance.request_timeout_ms,
                ))
                .connect_timeout(std::time::Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()?;

            let store = TokenStore::new(&config.profile)?;
            let outcome = OAuthStrategy::login_with_timeout(
                params,
                store,
                &http,
                !no_browser,
                std::time::Duration::from_millis(config.performance.request_timeout_ms),
            )
            .await?;

            println!("✓ Logged in (profile: {})", config.profile);
            if let Some(cid) = outcome.tokens.cloud_id.as_deref() {
                println!("  Cloud ID: {}", cid);
            }
            if !outcome.authorized_sites.is_empty() {
                println!("  Accessible sites:");
                for site in &outcome.authorized_sites {
                    let name = site.name.as_deref().unwrap_or("");
                    println!("    - {} ({}) {}", site.url, site.id, name);
                }
            }
            println!("  Scopes: {}", outcome.tokens.scopes.join(", "));
            Ok(())
        }
        AuthSubcommand::Logout => {
            // Clearing is driven by what is stored, not by the configured
            // method: a profile moved off OAuth keeps its persisted session,
            // and gating on the method would leave that credential unreachable.
            let config = atlassian_cli::Config::load_without_validation(
                config_path.as_ref(),
                profile.as_ref(),
                overrides,
            )?;
            clear_session(&TokenStore::new(&config.profile)?, &config.profile).await?;
            Ok(())
        }
        AuthSubcommand::Status => {
            // The configured method and the stored session are independent
            // facts: a profile switched away from OAuth still has whatever the
            // last login persisted. Report the method from config and the
            // session from the store, never inferring one from the other.
            let config = atlassian_cli::Config::load_without_validation(
                config_path.as_ref(),
                profile.as_ref(),
                overrides,
            )?;
            let method = config.auth.as_ref().map(|a| a.method());
            // Reported, not raised, and not in place of the rest: saying what
            // is configured and what is stored is this command's whole job, and
            // a store that would not answer is one of those answers — for a
            // profile whose credentials come from config it is not even the
            // interesting one. `self status` carries the same fact as data.
            let store = TokenStore::new(&config.profile)?;
            let keychain_unread =
                store.keychain() == atlassian_cli::auth::KeychainAccess::Forbidden;
            let (session, unreadable) = match store.load().await {
                Ok(session) => (session, None),
                Err(e) => (None, Some(e)),
            };

            // Above the match, so it holds for every arm: under the opt-out the
            // keychain was not consulted, and a session stored there before the
            // flag is one this report neither read nor cleared — including
            // where a file-backed session is found and printed below.
            if keychain_unread {
                println!(
                    "ATLASSIAN_NO_KEYCHAIN is set, so the keychain was not consulted; a session \
                     stored there before the flag is untouched — unset it for one run to see or \
                     clear what it holds."
                );
            }

            match (method, &session) {
                (Some(AuthMethod::OAuth), Some(loaded)) => {
                    let t = &loaded.tokens;
                    println!("✓ Logged in (profile: {})", config.profile);
                    println!("  Storage: {}", loaded.backend);
                    if let Some(cid) = &t.cloud_id {
                        println!("  Cloud ID: {}", cid);
                    }
                    println!("  Scopes: {}", t.scopes.join(", "));
                    let delta = t.seconds_until_expiry();
                    if delta > 0 {
                        println!("  Access token expires in: {}s ({}m)", delta, delta / 60);
                    } else {
                        println!("  Access token: EXPIRED ({}s ago)", -delta);
                    }
                    println!(
                        "  Refresh token: {}",
                        if t.refresh_token.is_some() {
                            "present"
                        } else {
                            "(none — re-login on expiry)"
                        }
                    );
                }
                (Some(AuthMethod::OAuth), None) => match &unreadable {
                    Some(e) => println!("Session unknown (profile: {}): {e}", config.profile),
                    // Not "not logged in" under the opt-out: the note above has
                    // already said the keychain went unread, and a session there
                    // is what `auth login` would shadow rather than reach.
                    None if keychain_unread => println!(
                        "No session in the file store (profile: {}).",
                        config.profile
                    ),
                    None => println!(
                        "Not logged in (profile: {}). Run `atlassian-cli auth login`.",
                        config.profile
                    ),
                },
                (Some(method), _) => {
                    println!(
                        "Profile '{}' uses '{}' auth — credentials are read from config/env, \
                         not a stored session.",
                        config.profile, method
                    );
                }
                (None, _) => println!("Profile '{}' has no auth configured.", config.profile),
            }

            // A session left behind by a previous OAuth configuration is a live
            // credential the current method never consults. Surface it so it can
            // be cleared rather than lingering unnoticed in the keychain — and
            // say when that could not be checked at all, because "no stale
            // session" is the reading a store nothing read must not produce.
            if !matches!(method, Some(AuthMethod::OAuth)) {
                if let Some(loaded) = &session {
                    println!(
                        "  Stale OAuth session present ({}) from an earlier configuration — \
                         run `atlassian-cli auth logout` to clear it.",
                        loaded.backend
                    );
                } else if unreadable.is_some() {
                    println!(
                        "  Whether a session is stored could not be read — see the error below."
                    );
                }
            }

            // The report prints first and in full: what could be established is
            // still the answer to what was asked, and it says on stdout where
            // it stops. What could not is the error — and the exit code, which
            // is the only part of a prose report a script can read.
            match unreadable {
                Some(e) => Err(e),
                None => Ok(()),
            }
        }
        AuthSubcommand::Refresh => {
            let config =
                atlassian_cli::Config::load(config_path.as_ref(), profile.as_ref(), overrides)?;
            let params = config.oauth_params()?;
            let store = TokenStore::new(&config.profile)?;
            let strategy = OAuthStrategy::resume_with_timeout(
                params,
                store,
                std::time::Duration::from_millis(config.performance.request_timeout_ms),
            )
            .await?;
            let refreshed = strategy.force_refresh().await?;
            println!("✓ Token refreshed (profile: {})", config.profile);
            println!(
                "  Access token now expires in: {}s",
                refreshed.seconds_until_expiry()
            );
            Ok(())
        }
    }
}

#[cfg(test)]
#[path = "test_support.rs"]
mod test_support;

#[cfg(test)]
#[path = "../examples/support/cli_reference.rs"]
mod cli_reference;

#[cfg(test)]
mod tests {
    use super::*;
    use atlassian_cli::auth::KeychainAccess;
    use atlassian_cli::dist::Installation;
    // Only the `#[cfg(unix)]` tests below deploy a skill.
    #[cfg(unix)]
    use atlassian_cli::dist::skill;

    #[test]
    fn cli_reference_matches_the_command_tree() {
        use clap::CommandFactory;

        assert_eq!(
            cli_reference::render(Cli::command()),
            include_str!("../docs/cli.md"),
            "regenerate docs/cli.md with cargo run --locked --example cli_reference > docs/cli.md"
        );
    }

    /// Scripts written against `--format html` keep working; the value it
    /// selected was always each product's stored representation.
    #[test]
    fn format_html_selects_the_stored_representation() {
        let jira = Cli::try_parse_from(["atlassian-cli", "jira", "get", "A-1", "--format", "html"]);
        assert!(matches!(
            jira.unwrap().command,
            Command::Jira(JiraCommand {
                subcommand: JiraSubcommand::Get {
                    format: JiraFormat::Adf,
                    ..
                }
            })
        ));
        let confluence = Cli::try_parse_from([
            "atlassian-cli",
            "confluence",
            "get",
            "1",
            "--format",
            "html",
        ]);
        assert!(matches!(
            confluence.unwrap().command,
            Command::Confluence(ConfluenceCommand {
                subcommand: ConfluenceSubcommand::Get {
                    format: ConfluenceFormat::Storage,
                    ..
                }
            })
        ));
    }

    /// `link remove` takes the issue pair or one link's id, and the handler's
    /// last arm rests on the parse refusing everything else. An argument
    /// relationship names its counterpart by string, so `debug_assert` is here
    /// too: a rename that dropped one silently would leave a delete reachable
    /// with an end of the link unnamed.
    #[test]
    fn link_remove_takes_the_issue_pair_or_an_id_and_never_both() {
        use clap::CommandFactory;
        Cli::command().debug_assert();

        const BIN: &str = "atlassian-cli";
        for args in [
            [BIN, "jira", "link", "remove", "A-1", "B-2"].as_slice(),
            &[
                BIN, "jira", "link", "remove", "A-1", "B-2", "--type", "Blocks",
            ],
            &[BIN, "jira", "link", "remove", "--id", "10001"],
        ] {
            assert!(Cli::try_parse_from(args).is_ok(), "{args:?}");
        }
        for args in [
            [BIN, "jira", "link", "remove", "A-1"].as_slice(),
            &[BIN, "jira", "link", "remove"],
            &[BIN, "jira", "link", "remove", "A-1", "--id", "10001"],
            &[BIN, "jira", "link", "remove", "A-1", "B-2", "--id", "10001"],
            &[
                BIN, "jira", "link", "remove", "--id", "10001", "--type", "Blocks",
            ],
        ] {
            assert!(Cli::try_parse_from(args).is_err(), "{args:?}");
        }
    }

    /// Where every home-managed artifact is kept by name, only the binary is
    /// to go and its path is known — the guard has nothing to protect.
    #[cfg(unix)]
    #[tokio::test]
    async fn an_unknown_home_still_removes_a_binary_nothing_else_is_asked_of() {
        let home = tempfile::tempdir().unwrap();
        let binary = home.path().join("bin").join("atlassian-cli");
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        std::fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
        let installation = Installation::at(binary.clone(), None);

        let report = self_uninstall(&installation, KeychainAccess::Allowed, true, true, false)
            .await
            .expect("nothing home-managed was asked for");

        assert_eq!(kinds(&report), vec!["binary"]);
        assert!(!binary.exists());
    }

    /// The binary is the only thing that knows where the home-managed
    /// artifacts are, so removing it with the home unknown leaves whatever was
    /// asked for with nothing to find it by. The refusal names what this run
    /// was asked to remove — a flag that keeps something is the run saying it
    /// is not at stake, and naming it anyway is a claim about the wrong thing.
    #[cfg(unix)]
    #[tokio::test]
    async fn an_unknown_home_refuses_naming_only_what_the_run_would_remove() {
        let home = tempfile::tempdir().unwrap();
        let binary = home.path().join("bin").join("atlassian-cli");
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        std::fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
        let installation = Installation::at(binary.clone(), None);

        for (keep_skill, keep_credentials, purge_config, named, unnamed) in [
            (
                false,
                true,
                true,
                vec!["the deployed skill", "the global config"],
                vec!["the stored credentials"],
            ),
            (
                true,
                false,
                false,
                vec!["the stored credentials"],
                vec!["the deployed skill", "the global config"],
            ),
        ] {
            let err = self_uninstall(
                &installation,
                KeychainAccess::Allowed,
                keep_skill,
                keep_credentials,
                purge_config,
            )
            .await
            .unwrap_err()
            .to_string();

            assert!(err.contains("home directory"), "{err}");
            for what in named {
                assert!(err.contains(what), "{what} unnamed: {err}");
            }
            for what in unnamed {
                assert!(!err.contains(what), "{what} named but kept: {err}");
            }
            assert!(binary.is_file(), "the binary went with the home unknown");
        }
    }

    /// A successor that answers `self --help` and one that does not. The
    /// second is what `--version <old>` installs, and the report has to say
    /// the deployed skill was left alone rather than name a subcommand that
    /// binary does not carry.
    #[cfg(unix)]
    #[test]
    fn a_successor_without_self_leaves_the_deployed_skill_alone() {
        for (script, expected) in [
            ("#!/bin/sh\nexit 0\n", "redeployed"),
            (
                "#!/bin/sh\necho \"error: unrecognized subcommand\" >&2\nexit 2\n",
                "unsupported",
            ),
        ] {
            let home = tempfile::tempdir().unwrap();
            let binary = home.path().join("bin").join("atlassian-cli");
            std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
            std::fs::write(&binary, script).unwrap();
            std::fs::set_permissions(&binary, std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .unwrap();
            let installation = Installation::at(binary, Some(home.path().to_path_buf()));
            let dir = installation.skill_dir().unwrap();
            skill::deploy(&dir).unwrap();

            assert_eq!(redeploy_skill(&installation), expected);
            // Either way the skill that was there is still there: the first
            // case redeployed it, the second declined to touch it.
            assert!(dir.join("SKILL.md").is_file());
        }
    }

    /// An installation whose every path lands under a temporary home, so the
    /// removal steps can run for real without touching the machine. Its binary
    /// is a placeholder file, which is why the tests that let `self_uninstall`
    /// reach the binary are `#[cfg(unix)]`: on Windows
    /// `self_replace::self_delete_at` removes a file by spawning a copy of it
    /// that waits for this process to exit, so it needs a real self-replace
    /// executable in the first place and would finish after the test either
    /// way. The steps before it are covered on every platform.
    fn installation(home: &std::path::Path) -> Installation {
        let binary = home.join("bin").join("atlassian-cli");
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        std::fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
        Installation::at(binary, Some(home.to_path_buf()))
    }

    #[cfg(unix)]
    fn write_config(installation: &Installation) {
        let dir = installation.config_dir().unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(installation.config_file().unwrap(), "[default.auth]\n").unwrap();
    }

    #[cfg(unix)]
    fn kinds(report: &serde_json::Value) -> Vec<&str> {
        report["removed"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["kind"].as_str().unwrap())
            .collect()
    }

    #[cfg(unix)]
    fn kept(report: &serde_json::Value) -> Vec<&str> {
        report["kept"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry.as_str().unwrap())
            .collect()
    }

    /// `--keep-credentials` is what keeps the keychain out of these tests: it
    /// is the one step whose backend belongs to the machine.
    #[cfg(unix)]
    #[tokio::test]
    async fn uninstall_removes_the_skill_and_the_binary_and_keeps_the_config() {
        let home = tempfile::tempdir().unwrap();
        let installation = installation(home.path());
        write_config(&installation);
        skill::deploy(&installation.skill_dir().unwrap()).unwrap();

        let report = self_uninstall(&installation, KeychainAccess::Allowed, false, true, false)
            .await
            .unwrap();

        assert_eq!(kinds(&report), vec!["skill", "binary"]);
        assert_eq!(kept(&report), vec!["credentials", "config"]);
        assert!(!installation.binary().exists());
        assert!(!installation.skill_dir().unwrap().exists());
        assert!(installation.config_file().unwrap().is_file());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn keep_skill_leaves_it_and_says_so() {
        let home = tempfile::tempdir().unwrap();
        let installation = installation(home.path());
        skill::deploy(&installation.skill_dir().unwrap()).unwrap();

        let report = self_uninstall(&installation, KeychainAccess::Allowed, true, true, false)
            .await
            .unwrap();

        assert_eq!(kinds(&report), vec!["binary"]);
        assert!(kept(&report).contains(&"skill"));
        assert!(installation.skill_dir().unwrap().is_dir());
    }

    /// `credentials.json` sits in the directory `--purge-config` removes, so a
    /// whole-directory delete would take it whatever `--keep-credentials` said.
    #[cfg(unix)]
    #[tokio::test]
    async fn purge_config_leaves_the_credentials_file_alone() {
        let home = tempfile::tempdir().unwrap();
        let installation = installation(home.path());
        write_config(&installation);
        let credentials = installation.credentials_file().unwrap();
        std::fs::write(&credentials, "{}").unwrap();

        let report = self_uninstall(&installation, KeychainAccess::Allowed, true, true, true)
            .await
            .unwrap();

        assert!(credentials.is_file(), "the token file was purged anyway");
        assert!(!installation.config_file().unwrap().exists());
        // The directory still holds the token file, so it stays — and says so
        // rather than going unmentioned.
        assert!(installation.config_dir().unwrap().is_dir());
        assert!(kept(&report).contains(&"config-directory"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn purge_config_removes_a_directory_it_empties() {
        let home = tempfile::tempdir().unwrap();
        let installation = installation(home.path());
        write_config(&installation);

        let report = self_uninstall(&installation, KeychainAccess::Allowed, true, true, true)
            .await
            .unwrap();

        assert!(!installation.config_dir().unwrap().exists());
        assert!(!kept(&report).contains(&"config-directory"));
    }

    /// A config directory the user redirected is as definite a "stays" as a
    /// full one: `remove_dir` answers `NotADirectory` there, and reading that
    /// as a failure stops the uninstall after the tokens and the skill are
    /// already gone.
    #[cfg(unix)]
    #[tokio::test]
    async fn purge_config_keeps_a_directory_the_user_redirected() {
        let home = tempfile::tempdir().unwrap();
        let installation = installation(home.path());
        let real = home.path().join("dotfiles");
        std::fs::create_dir_all(&real).unwrap();
        let dir = installation.config_dir().unwrap();
        std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&real, &dir).unwrap();
        std::fs::write(dir.join("config.toml"), "[default]\n").unwrap();

        let report = self_uninstall(&installation, KeychainAccess::Allowed, true, true, true)
            .await
            .unwrap();

        assert!(kept(&report).contains(&"config-directory"));
        assert!(!installation.binary().exists());
        assert!(
            std::fs::symlink_metadata(&dir)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        // The file this tool wrote goes, through the link: it is at the path
        // this installation owns whatever the directory points at.
        assert!(!real.join("config.toml").exists());
    }

    /// A file this tool did not write keeps the directory alive; deleting it
    /// would be taking something that is not ours.
    #[cfg(unix)]
    #[tokio::test]
    async fn purge_config_spares_a_file_this_tool_did_not_write() {
        let home = tempfile::tempdir().unwrap();
        let installation = installation(home.path());
        write_config(&installation);
        let stray = installation.config_dir().unwrap().join("notes.txt");
        std::fs::write(&stray, "mine").unwrap();

        self_uninstall(&installation, KeychainAccess::Allowed, true, true, true)
            .await
            .unwrap();

        assert!(stray.is_file());
    }

    /// Each step is irreversible, so a failure has to name what it already did.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_binary_that_cannot_be_removed_reports_what_already_went() {
        crate::test_support::require_enforced_modes();
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let installation = installation(home.path());
        let skill_dir = installation.skill_dir().unwrap();
        skill::deploy(&skill_dir).unwrap();

        let bin_dir = installation.binary().parent().unwrap().to_path_buf();
        std::fs::set_permissions(&bin_dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let err = self_uninstall(&installation, KeychainAccess::Allowed, false, true, false)
            .await
            .unwrap_err();
        std::fs::set_permissions(&bin_dir, std::fs::Permissions::from_mode(0o700)).unwrap();

        let message = format!("{err:#}");
        assert!(message.contains("already removed"), "{message}");
        assert!(
            message.contains(&skill_dir.display().to_string()),
            "the skill it had already removed is not named: {message}"
        );
    }

    /// The credential step needs a keychain that answers, and the machine's
    /// belongs to the machine. A mock store is installed once for the whole bin
    /// test process; `ensure_store_installed` keeps whichever store is already
    /// set, so every test below enumerates against this one.
    fn mock_keychain() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
        });
    }

    fn write_credentials(installation: &Installation, profiles: &[&str]) {
        let file = installation.credentials_file().unwrap();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let body: serde_json::Map<String, serde_json::Value> = profiles
            .iter()
            .map(|profile| {
                (
                    (*profile).to_string(),
                    serde_json::json!({
                        "access_token": "a",
                        "refresh_token": null,
                        "expires_at_unix": 0,
                        "scopes": [],
                        "cloud_id": null
                    }),
                )
            })
            .collect();
        std::fs::write(&file, serde_json::to_vec(&body).unwrap()).unwrap();
    }

    /// The file names the profiles it holds, so enumerating has to happen
    /// before it is deleted or those profiles go unreported — which is what the
    /// whole `removed` record exists to prevent.
    #[cfg(unix)]
    #[tokio::test]
    async fn every_file_backed_profile_is_named_in_what_was_removed() {
        mock_keychain();
        let home = tempfile::tempdir().unwrap();
        let installation = installation(home.path());
        write_credentials(&installation, &["default", "work"]);

        let report = self_uninstall(&installation, KeychainAccess::Allowed, true, false, false)
            .await
            .unwrap();

        let targets: Vec<&str> = report["removed"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["kind"] == "credentials")
            .map(|entry| entry["target"].as_str().unwrap())
            .collect();
        assert!(targets.contains(&"profile:default"), "{targets:?}");
        assert!(targets.contains(&"profile:work"), "{targets:?}");
        assert!(!installation.credentials_file().unwrap().exists());
        // Not by index: the shared mock keychain accumulates every profile any
        // test in this binary touched, and they are reported sorted.
        let reported = report["credentials"]["profiles"].as_array().unwrap();
        assert!(reported.iter().any(|p| p == "default"), "{reported:?}");
    }

    /// A profile the keychain alone holds is one the uninstall reports cleared,
    /// and the report is checked against the keychain rather than against
    /// itself: saying a token went while it is still there is what leaves a
    /// refresh token on a machine the binary has left.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_keychain_backed_profile_is_gone_from_the_keychain_afterwards() {
        use atlassian_cli::auth::{TokenSet, TokenStore};
        use secrecy::SecretString;

        mock_keychain();
        const PROFILE: &str = "uninstall-clears-the-keychain";
        let home = tempfile::tempdir().unwrap();
        let installation = installation(home.path());
        let file = installation.credentials_file().unwrap();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();

        // The save lands in the keychain and clears the file, so this profile
        // is named by the keychain alone.
        let backend = TokenStore::at(PROFILE, file.clone(), KeychainAccess::Allowed)
            .save(&TokenSet {
                access_token: SecretString::new("access".into()),
                refresh_token: Some(SecretString::new("refresh".into())),
                expires_at_unix: 4_102_444_800,
                scopes: vec!["read:jira-work".to_string()],
                cloud_id: None,
            })
            .await
            .unwrap();
        assert_eq!(backend, atlassian_cli::auth::TokenStorageBackend::Keyring);

        let report = self_uninstall(&installation, KeychainAccess::Allowed, true, false, false)
            .await
            .unwrap();

        let targets: Vec<&str> = report["removed"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|entry| entry["target"].as_str())
            .collect();
        assert!(
            targets.contains(&format!("profile:{PROFILE}").as_str()),
            "{targets:?}"
        );
        // Read back through a store with the same backends: the mock keychain
        // keeps naming an entry it no longer holds, so what is checked is
        // whether the credential is still readable, which is what was claimed.
        let left = TokenStore::at(PROFILE, file, KeychainAccess::Allowed)
            .load()
            .await
            .unwrap();
        assert!(
            left.is_none(),
            "reported cleared, still readable from {}",
            left.unwrap().backend
        );
    }

    /// `ATLASSIAN_NO_KEYCHAIN` is a per-environment setting, so this asserts the
    /// branch through the enumeration outcome rather than by setting it.
    #[tokio::test]
    async fn an_enumeration_that_did_not_happen_refuses_before_the_binary_goes() {
        let home = tempfile::tempdir().unwrap();
        let installation = installation(home.path());
        write_credentials(&installation, &["default"]);

        let refusal =
            clear_stored_tokens_refusal(&atlassian_cli::auth::KeyringEnumeration::Skipped);
        assert!(
            refusal.is_some(),
            "a forbidden look must not pass for a clear"
        );
        assert!(
            clear_stored_tokens_refusal(&atlassian_cli::auth::KeyringEnumeration::Failed(
                "locked".to_string()
            ))
            .is_some_and(|message| message.contains("locked")),
            "a store that would not answer must name why"
        );
        // A build with no store never wrote to one, so there is nothing to miss.
        assert!(
            clear_stored_tokens_refusal(&atlassian_cli::auth::KeyringEnumeration::Unsupported)
                .is_none()
        );
        assert!(
            clear_stored_tokens_refusal(&atlassian_cli::auth::KeyringEnumeration::Listed).is_none()
        );
        assert!(installation.binary().exists());
    }

    /// `exists` resolves the link, so a dangling one reads as nothing there —
    /// the exact state `skill::remove` classifies with `symlink_metadata` in
    /// order to clean up. A guard at the call site would undo that.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_dangling_skill_link_is_removed_rather_than_stepped_over() {
        let home = tempfile::tempdir().unwrap();
        let installation = installation(home.path());
        let dir = installation.skill_dir().unwrap();
        std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(home.path().join("gone"), &dir).unwrap();

        let report = self_uninstall(&installation, KeychainAccess::Allowed, false, true, false)
            .await
            .unwrap();

        assert!(
            std::fs::symlink_metadata(&dir).is_err(),
            "the link outlived the tool that put it there"
        );
        assert!(kinds(&report).contains(&"skill"));
    }

    /// Unlinking a link clears the name, not the tokens. Reporting that as a
    /// cleared session and then removing the binary is how a token ends up with
    /// nothing left that knows where it is.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_credentials_link_stops_the_uninstall_before_the_binary_goes() {
        mock_keychain();
        let home = tempfile::tempdir().unwrap();
        let installation = installation(home.path());
        let link = installation.credentials_file().unwrap();
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        let real = home.path().join("tokens.json");
        std::fs::write(&real, r#"{"default":{}}"#).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let err = self_uninstall(&installation, KeychainAccess::Allowed, true, false, false)
            .await
            .unwrap_err();

        assert!(format!("{err:#}").contains("not a regular file"), "{err:#}");
        assert_eq!(std::fs::read_to_string(&real).unwrap(), r#"{"default":{}}"#);
        assert!(installation.binary().exists());
    }

    /// A directory that will not open answers every question about what is
    /// inside it with an error, and `exists` renders that as "nothing there".
    /// Taking it that way removes the binary and reports a token gone that was
    /// never reachable to clear.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_credentials_file_that_cannot_be_reached_stops_the_uninstall() {
        crate::test_support::require_enforced_modes();
        use std::os::unix::fs::PermissionsExt;
        mock_keychain();
        let home = tempfile::tempdir().unwrap();
        let installation = installation(home.path());
        write_credentials(&installation, &["default"]);
        let dir = installation.config_dir().unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000)).unwrap();

        let outcome =
            self_uninstall(&installation, KeychainAccess::Allowed, true, false, false).await;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();

        assert!(
            outcome.is_err(),
            "an unreachable token file passed for an absent one"
        );
        assert!(installation.binary().exists());
        assert!(installation.credentials_file().unwrap().is_file());
    }

    /// The same rule one step out: a skill directory that cannot be stat'd is
    /// not an absent one, and stepping over it removes the binary and reports
    /// a clean uninstall with the skill still deployed.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_skill_directory_that_cannot_be_reached_stops_the_uninstall() {
        crate::test_support::require_enforced_modes();
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let installation = installation(home.path());
        let dir = installation.skill_dir().unwrap();
        skill::deploy(&dir).unwrap();
        let parent = dir.parent().unwrap().to_path_buf();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o000)).unwrap();

        let outcome =
            self_uninstall(&installation, KeychainAccess::Allowed, false, true, false).await;
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();

        assert!(
            outcome.is_err(),
            "an unreachable skill passed for an absent one"
        );
        assert!(installation.binary().exists());
    }

    /// A path the platform accepts and JSON cannot spell. `json!` serializes a
    /// `Path` by unwrapping, so a report built from one would panic instead of
    /// printing the single-line error object every failure here promises.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_report_names_a_path_that_is_not_utf8() {
        use std::os::unix::ffi::OsStringExt;
        let home = tempfile::tempdir().unwrap();
        // Not created: filesystems differ on whether they will take the name,
        // and what is under test is the report, not the removal.
        let binary = home
            .path()
            .join(std::ffi::OsString::from_vec(b"bin\xff".to_vec()));
        let installation = Installation::at(binary.clone(), Some(home.path().to_path_buf()));

        let report = self_uninstall(&installation, KeychainAccess::Allowed, true, true, false)
            .await
            .unwrap();

        assert_eq!(report["binary"], binary.display().to_string());
    }

    /// A stored entry that will not parse is still an entry to remove, and
    /// removing it does not need it parsed. Letting the read's failure out
    /// first leaves the credential exactly where it was.
    #[tokio::test]
    async fn logout_clears_an_entry_that_cannot_be_read_back() {
        mock_keychain();
        let home = tempfile::tempdir().unwrap();
        let file = home.path().join("credentials.json");
        let store =
            atlassian_cli::auth::TokenStore::at("logout-corrupt", file, KeychainAccess::Allowed);

        keyring_core::Entry::new("atlassian-cli", "logout-corrupt")
            .unwrap()
            .set_password("not a token document")
            .unwrap();

        clear_session(&store, "logout-corrupt").await.unwrap();

        assert!(matches!(
            keyring_core::Entry::new("atlassian-cli", "logout-corrupt")
                .unwrap()
                .get_password(),
            Err(keyring_core::Error::NoEntry)
        ));
    }

    #[test]
    fn an_error_with_nothing_removed_behind_it_is_left_as_it_is() {
        let error = anyhow::anyhow!("could not remove the binary");
        assert_eq!(
            format!("{:#}", already_removed(error, &[])),
            "could not remove the binary"
        );
    }
}
