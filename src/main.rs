use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Clone, Copy, Default, ValueEnum)]
enum OutputFormat {
    #[default]
    Html,
    Markdown,
}

#[derive(Parser)]
#[command(name = "atlassian-cli", version, about = "CLI for Atlassian Jira and Confluence", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    #[arg(long, help = "Config file path")]
    config: Option<PathBuf>,

    #[arg(long, help = "Profile name")]
    profile: Option<String>,

    #[arg(long, env = "ATLASSIAN_DOMAIN")]
    domain: Option<String>,

    #[arg(long, env = "ATLASSIAN_EMAIL")]
    email: Option<String>,

    #[arg(long, env = "ATLASSIAN_API_TOKEN")]
    token: Option<String>,

    #[arg(long, env = "ATLASSIAN_CLIENT_ID")]
    client_id: Option<String>,

    #[arg(long, env = "ATLASSIAN_CLIENT_SECRET")]
    client_secret: Option<String>,

    #[arg(long, env = "ATLASSIAN_CLOUD_ID")]
    cloud_id: Option<String>,

    #[arg(long, help = "Pretty-print JSON output")]
    pretty: bool,

    #[arg(short, long, action = clap::ArgAction::Count, help = "Verbose logging")]
    verbose: u8,
}

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

#[derive(Subcommand)]
enum Command {
    Jira(JiraCommand),
    Confluence(ConfluenceCommand),
    Config(ConfigCommand),
    Auth(AuthCommand),
    #[command(name = "self", about = "Inspect, update, or remove this installation")]
    Selfcmd(SelfCommand),
    #[command(about = "Generate a shell completion script on stdout")]
    Completions {
        #[arg(value_enum, help = "Target shell")]
        shell: clap_complete::Shell,
    },
}

#[derive(Parser)]
struct SelfCommand {
    #[command(subcommand)]
    subcommand: SelfSubcommand,
}

#[derive(Subcommand)]
enum SelfSubcommand {
    /// Report what this installation is and where its files are
    Status,
    /// Replace this binary with a published release
    Update {
        /// Install this version instead of the latest. Named explicitly, so it may go back.
        #[arg(long, value_name = "VER")]
        version: Option<String>,
        /// Replace the binary even when it already reports the target version
        #[arg(long)]
        force: bool,
        /// Additionally hold the archive to the build provenance the release attested
        #[arg(long)]
        verify_attestations: bool,
    },
    /// Install or remove the Claude Code skill this binary carries
    Skill {
        #[command(subcommand)]
        action: SelfSkillAction,
    },
    /// Remove this binary, the skill it deployed, and the tokens it stored
    Uninstall {
        /// Confirm the removal
        #[arg(long)]
        yes: bool,
        /// Leave the deployed skill in place
        #[arg(long)]
        keep_skill: bool,
        /// Leave stored OAuth tokens in the keychain and the credentials file
        #[arg(long)]
        keep_credentials: bool,
        /// Also remove the global configuration directory
        #[arg(long)]
        purge_config: bool,
    },
}

#[derive(Subcommand)]
enum SelfSkillAction {
    /// Write the skill this binary carries into ~/.claude/skills
    Install,
    /// Remove the deployed skill
    Remove {
        /// Confirm the removal
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Parser)]
struct AuthCommand {
    #[command(subcommand)]
    subcommand: AuthSubcommand,
}

#[derive(Subcommand)]
enum AuthSubcommand {
    /// Start the OAuth 3LO flow and persist tokens for the active profile.
    Login {
        #[arg(long, help = "Print the authorize URL instead of opening a browser")]
        no_browser: bool,
    },
    /// Discard stored OAuth tokens for the active profile.
    Logout,
    /// Show the active profile's stored token status (identity, expiry, scopes).
    Status,
    /// Force-refresh the access_token using the stored refresh_token.
    Refresh,
}

#[derive(Parser)]
struct JiraCommand {
    #[command(subcommand)]
    subcommand: JiraSubcommand,
}

#[derive(Subcommand)]
enum JiraSubcommand {
    /// Fetch a single issue by key (fields filtered, ADF rendered)
    Get {
        issue_key: String,
        #[arg(
            long,
            value_delimiter = ',',
            help = "Fields to return (default: essential set + configured custom fields; use *all for the full issue)"
        )]
        fields: Option<Vec<String>>,
        #[arg(long, value_enum, default_value = "html", help = "ADF content format")]
        format: OutputFormat,
    },
    /// Search issues with JQL; the configured project filter is auto-injected
    Search {
        jql: String,
        #[arg(long, default_value = "100", help = "Results per page")]
        limit: u32,
        #[arg(long, help = "Fetch all results via token pagination")]
        all: bool,
        #[arg(long, help = "Stream as JSONL (requires --all)")]
        stream: bool,
        #[arg(long, value_delimiter = ',', help = "Fields to return")]
        fields: Option<Vec<String>>,
        #[arg(long, value_enum, default_value = "html", help = "ADF content format")]
        format: OutputFormat,
    },
    /// Create an issue (plain-text description auto-converts to ADF)
    Create {
        project: String,
        summary: String,
        issue_type: String,
        #[arg(long)]
        description: Option<String>,
    },
    /// Update an issue's fields from a JSON object (e.g. '{"summary":"..."}')
    Update { issue_key: String, fields: String },
    /// Permanently delete an issue (irreversible — requires --yes)
    Delete {
        issue_key: String,
        /// Confirm the irreversible deletion
        #[arg(long)]
        yes: bool,
        /// Also delete subtasks (Jira rejects the call otherwise when present)
        #[arg(long)]
        delete_subtasks: bool,
    },
    /// Add, update, or list comments on an issue
    Comment {
        #[command(subcommand)]
        action: CommentAction,
    },
    /// Apply or list workflow transitions for an issue
    Transition {
        #[command(subcommand)]
        action: TransitionAction,
    },
    /// Create, remove, or list issue links (and list link types)
    Link {
        #[command(subcommand)]
        action: LinkAction,
    },
    /// Add, update, list, or remove worklog (time-tracking) entries
    Worklog {
        #[command(subcommand)]
        action: WorklogAction,
    },
    /// Start watching, stop watching, or list watchers on an issue
    Watcher {
        #[command(subcommand)]
        action: WatcherAction,
    },
    /// Query global metadata (issue types, priorities, statuses, labels)
    List {
        #[command(subcommand)]
        action: ListAction,
    },
    /// List agile boards for a project
    Board {
        #[command(subcommand)]
        action: BoardAction,
    },
    /// List sprints, or move issues between a sprint and the backlog
    Sprint {
        #[command(subcommand)]
        action: SprintAction,
    },
    /// Assign issues to an epic, or remove them from their epics
    Epic {
        #[command(subcommand)]
        action: EpicAction,
    },
}

#[derive(Subcommand)]
enum CommentAction {
    /// Add a comment to an issue
    Add { issue_key: String, text: String },
    /// Update an existing comment
    Update {
        issue_key: String,
        comment_id: String,
        text: String,
    },
    /// List comments on an issue
    List {
        issue_key: String,
        #[arg(long, value_enum, default_value = "html", help = "ADF content format")]
        format: OutputFormat,
    },
    /// Delete a comment by id
    Delete {
        issue_key: String,
        comment_id: String,
    },
}

#[derive(Subcommand)]
enum TransitionAction {
    /// Apply a transition to an issue
    Apply {
        issue_key: String,
        transition_id: String,
    },
    /// List available transitions for an issue
    List { issue_key: String },
}

#[derive(Subcommand)]
enum LinkAction {
    /// List available link types
    Types,
    /// Create a link between two issues
    Add {
        /// Source issue key (outward side: "A blocks B" → A)
        source: String,
        /// Target issue key (inward side: "A blocks B" → B)
        target: String,
        /// Link type name
        #[arg(long = "type", default_value = "Relates")]
        link_type: String,
        /// Comment to add with the link
        #[arg(long)]
        comment: Option<String>,
    },
    /// Remove a link between two issues
    Remove {
        /// Source issue key
        source: String,
        /// Target issue key
        target: String,
        /// Link type (required when multiple link types exist between the pair)
        #[arg(long = "type")]
        link_type: Option<String>,
    },
    /// List links on an issue
    List {
        /// Issue key
        issue_key: String,
    },
}

#[derive(Subcommand)]
enum WorklogAction {
    /// Add a worklog entry to an issue
    Add {
        /// Issue key
        issue_key: String,
        /// Time spent (e.g., "2h 30m", "1d", "45m")
        time_spent: String,
        /// Comment describing the work
        #[arg(long)]
        comment: Option<String>,
        /// Start time in ISO 8601 format (defaults to now)
        #[arg(long)]
        started: Option<String>,
    },
    /// List worklog entries on an issue
    List {
        /// Issue key
        issue_key: String,
    },
    /// Update a worklog entry
    Update {
        /// Issue key
        issue_key: String,
        /// Worklog ID
        worklog_id: String,
        /// New time spent
        time_spent: String,
        /// Updated comment
        #[arg(long)]
        comment: Option<String>,
    },
    /// Remove a worklog entry
    Remove {
        /// Issue key
        issue_key: String,
        /// Worklog ID
        worklog_id: String,
    },
}

#[derive(Subcommand)]
enum WatcherAction {
    /// Start watching an issue (adds current user)
    Add {
        /// Issue key
        issue_key: String,
    },
    /// Stop watching an issue (removes current user)
    Remove {
        /// Issue key
        issue_key: String,
    },
    /// List watchers on an issue
    List {
        /// Issue key
        issue_key: String,
    },
}

#[derive(Subcommand)]
enum ListAction {
    /// List available issue types
    Types,
    /// List available priorities
    Priorities,
    /// List available statuses
    Statuses,
    /// List available labels
    Labels,
}

#[derive(Subcommand)]
enum BoardAction {
    /// List boards for a project
    List {
        /// Project key or ID
        #[arg(long)]
        project: String,
    },
}

#[derive(Subcommand)]
enum SprintAction {
    /// List sprints on a board
    List {
        /// Board ID
        #[arg(long, group = "board_source")]
        board: Option<u64>,
        /// Project key (auto-resolves board)
        #[arg(long, group = "board_source")]
        project: Option<String>,
        /// Sprint state filter
        #[arg(long, default_value = "active,future")]
        state: String,
    },
    /// Move issues to a sprint
    Move {
        /// Sprint ID
        sprint_id: u64,
        /// Issue keys to move
        #[arg(required = true)]
        issues: Vec<String>,
    },
    /// Move issues to the backlog
    Backlog {
        /// Issue keys to move
        #[arg(required = true)]
        issues: Vec<String>,
    },
}

#[derive(Subcommand)]
enum EpicAction {
    /// Assign issues to an epic
    Assign {
        /// Epic issue key
        epic_key: String,
        /// Issue keys to assign
        #[arg(required = true)]
        issues: Vec<String>,
    },
    /// Remove issues from their epics
    Unassign {
        /// Issue keys to unassign
        #[arg(required = true)]
        issues: Vec<String>,
    },
}

#[derive(Parser)]
struct ConfluenceCommand {
    #[command(subcommand)]
    subcommand: ConfluenceSubcommand,
}

#[derive(Subcommand)]
enum ConfluenceSubcommand {
    /// Search pages with CQL; the configured space filter is auto-injected
    Search {
        query: String,
        #[arg(
            long,
            default_value = "50",
            help = "Results per page (capped at 50 by the body-expanding search API). With --all, controls first-page batch size"
        )]
        limit: u32,
        #[arg(long, help = "Fetch all results via cursor pagination")]
        all: bool,
        #[arg(long, help = "Stream as JSONL (requires --all)")]
        stream: bool,
        #[arg(
            long,
            value_delimiter = ',',
            help = "Expand fields (e.g., body.storage,ancestors)"
        )]
        expand: Option<Vec<String>>,
        #[arg(long, value_enum, default_value = "html", help = "Body content format")]
        format: OutputFormat,
    },
    /// Fetch a single page by ID (body rendered as HTML or markdown)
    Get {
        page_id: String,
        #[arg(long, value_enum, default_value = "html", help = "Body content format")]
        format: OutputFormat,
    },
    /// Create a page from storage-format HTML content
    Create {
        space: String,
        title: String,
        content: String,
        /// Parent page id to nest under (omit to create at the space root)
        #[arg(long)]
        parent: Option<String>,
    },
    /// Update a page's title and storage-format HTML content
    Update {
        page_id: String,
        title: String,
        content: String,
        /// Parent page id to re-parent under (omit to keep the current parent)
        #[arg(long)]
        parent: Option<String>,
    },
    /// List the direct child pages of a page (metadata only)
    Children { page_id: String },
    /// Read a page's comments and threads, or write footer comments
    Comment {
        #[command(subcommand)]
        action: ConfluenceCommentAction,
    },
    /// List, add, or remove labels on a page
    Label {
        #[command(subcommand)]
        action: ConfluenceLabelAction,
    },
    /// List, set, or delete content properties (structured JSON metadata) on a page
    Property {
        #[command(subcommand)]
        action: ConfluencePropertyAction,
    },
    /// List spaces, or fetch a single space by key
    Space {
        #[command(subcommand)]
        action: ConfluenceSpaceAction,
    },
    /// List attachments on a page, or upload a file
    Attachment {
        #[command(subcommand)]
        action: ConfluenceAttachmentAction,
    },
    /// Move a page to the trash (recoverable — requires --yes)
    Delete {
        page_id: String,
        /// Confirm the deletion
        #[arg(long)]
        yes: bool,
    },
}

/// Which of Confluence's two comment families an id or a listing refers to.
#[derive(Clone, Copy, ValueEnum)]
enum CommentLocation {
    Footer,
    Inline,
}

impl From<CommentLocation> for atlassian_cli::confluence::CommentFamily {
    fn from(location: CommentLocation) -> Self {
        use atlassian_cli::confluence::CommentFamily;
        match location {
            CommentLocation::Footer => CommentFamily::Footer,
            CommentLocation::Inline => CommentFamily::Inline,
        }
    }
}

#[derive(Subcommand)]
enum ConfluenceCommentAction {
    /// List comments on a page, replies included
    List {
        page_id: String,
        #[arg(
            long,
            value_enum,
            help = "Comment family to list (default: both footer and inline)"
        )]
        location: Option<CommentLocation>,
        /// List only top-level comments, leaving their replies unfetched
        #[arg(long)]
        roots_only: bool,
        #[arg(long, value_enum, default_value = "html", help = "Body content format")]
        format: OutputFormat,
    },
    /// Fetch a single comment by id
    Get {
        comment_id: String,
        #[arg(
            long,
            value_enum,
            default_value = "footer",
            help = "Comment family the id belongs to"
        )]
        location: CommentLocation,
        #[arg(long, value_enum, default_value = "html", help = "Body content format")]
        format: OutputFormat,
    },
    /// List every reply below a comment
    Replies {
        comment_id: String,
        #[arg(
            long,
            value_enum,
            default_value = "footer",
            help = "Comment family the id belongs to"
        )]
        location: CommentLocation,
        #[arg(long, value_enum, default_value = "html", help = "Body content format")]
        format: OutputFormat,
    },
    /// Add a footer comment to a page (storage-format HTML body)
    Add {
        page_id: String,
        /// Comment body (storage-format HTML; plain text is valid too)
        body: String,
        /// Reply to an existing comment instead of posting a top-level one
        #[arg(long = "reply-to")]
        reply_to: Option<String>,
    },
    /// Update a footer comment's body
    Update {
        comment_id: String,
        /// New comment body (storage-format HTML)
        body: String,
    },
    /// Delete a footer comment by id
    Delete { comment_id: String },
}

#[derive(Subcommand)]
enum ConfluenceLabelAction {
    /// List labels on a page
    List { page_id: String },
    /// Add a label to a page
    Add { page_id: String, label: String },
    /// Remove a label from a page
    Remove { page_id: String, label: String },
}

#[derive(Subcommand)]
enum ConfluencePropertyAction {
    /// List content properties on a page
    List { page_id: String },
    /// Create or update a content property (value is a JSON literal)
    Set {
        page_id: String,
        key: String,
        /// Property value as a JSON literal (e.g. '{"state":"done"}', '42', '"text"')
        value: String,
    },
    /// Delete a content property by key
    Delete { page_id: String, key: String },
}

#[derive(Subcommand)]
enum ConfluenceSpaceAction {
    /// List spaces visible to you
    List,
    /// Get a single space by key
    Get { space_key: String },
}

#[derive(Subcommand)]
enum ConfluenceAttachmentAction {
    /// List attachments on a page
    List { page_id: String },
    /// Upload a local file as an attachment (creates, or versions by filename)
    Upload {
        page_id: String,
        /// Path to the local file to upload
        file: String,
        /// Optional version comment recorded with the upload
        #[arg(long)]
        comment: Option<String>,
        /// Mark as a minor edit (suppresses watcher notifications on re-upload)
        #[arg(long)]
        minor: bool,
        /// Override the Content-Type (default: mapped from the file extension)
        #[arg(long = "content-type")]
        content_type: Option<String>,
    },
}

#[derive(Parser)]
struct ConfigCommand {
    #[command(subcommand)]
    subcommand: ConfigSubcommand,
}

#[derive(Subcommand)]
enum ConfigSubcommand {
    /// Create a starter config file at the global or project location.
    Init {
        #[arg(
            long,
            help = "Write to ~/.config/atlassian-cli/config.toml instead of ./.atlassian.toml"
        )]
        global: bool,
    },
    /// Print the resolved config (secrets masked).
    Show,
    /// List config file paths and environment variable status.
    List,
    /// Open the active config file in $EDITOR.
    Edit {
        #[arg(
            long,
            help = "Edit the global config even when a project config exists"
        )]
        global: bool,
    },
    /// Print the path of the active config file.
    Path {
        #[arg(
            long,
            help = "Print the global config path even when a project config exists"
        )]
        global: bool,
    },
    /// Validate configured credentials against Atlassian auth/API endpoints.
    Validate,
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
            let result = handle_jira(cmd, &client).await?;
            output_json(&result, cli.pretty);
            Ok(())
        }
        Command::Confluence(cmd) => {
            let config =
                atlassian_cli::Config::load(config_path.as_ref(), profile.as_ref(), overrides)?;

            let client = atlassian_cli::ApiClient::new(config).await?;
            let result = handle_confluence(cmd, &client).await?;
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
        SelfSubcommand::Status => self_status(&installation).await,
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
            self_uninstall(&installation, keep_skill, keep_credentials, purge_config).await
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
) -> Result<serde_json::Value> {
    use atlassian_cli::dist;

    let credentials_file = installation.credentials_file();
    let stored = stored_profiles_of(installation).await;
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
    keep_skill: bool,
    keep_credentials: bool,
    purge_config: bool,
) -> Result<serde_json::Value> {
    use atlassian_cli::dist::skill;

    // Before anything goes. Without a home directory the skill and the global
    // config cannot be located, and every step below would skip them silently
    // and then take the binary that knows where they are — the same reason a
    // keychain that will not answer refuses here rather than proceeding.
    if installation.skill_dir().is_none() || installation.config_dir().is_none() {
        anyhow::bail!(
            "Cannot locate the home directory, so the deployed skill and the global config \
             cannot be found — removing the binary would leave them behind with nothing that \
             knows where they are. Set HOME and re-run."
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
        clear_stored_tokens(installation, &mut removed)
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
    removed: &mut Vec<serde_json::Value>,
) -> Result<serde_json::Value> {
    // Enumerate before removing anything: the file is where the file-backed
    // profiles are named, so deleting it first would leave them out of the
    // report even though they were cleared.
    let stored = stored_profiles_of(installation).await;

    if let Some(file) = installation.credentials_file()
        && atlassian_cli::auth::remove_credentials_file(&file)?
    {
        record(removed, "credentials", display_path(&file));
    }

    if let Some(refusal) = clear_stored_tokens_refusal(&stored.keyring) {
        anyhow::bail!(refusal);
    }

    for profile in &stored.profiles {
        token_store(installation, profile)?.delete().await?;
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
/// rather than the running machine's.
async fn stored_profiles_of(
    installation: &atlassian_cli::dist::Installation,
) -> atlassian_cli::auth::StoredProfiles {
    let file = installation
        .credentials_file()
        .unwrap_or_else(|| std::path::PathBuf::from(atlassian_cli::auth::CREDENTIALS_FILE));
    atlassian_cli::auth::stored_profiles(&file).await
}

fn token_store(
    installation: &atlassian_cli::dist::Installation,
    profile: &str,
) -> Result<atlassian_cli::auth::TokenStore> {
    let file = installation
        .credentials_file()
        .ok_or_else(|| anyhow::anyhow!("Failed to determine home directory"))?;
    Ok(atlassian_cli::auth::TokenStore::at(profile, file))
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
    if atlassian_cli::auth::keychain_opt_out() {
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
            let path = atlassian_cli::Config::init_config(global)?;
            println!("Created config file: {:?}", path);
            println!("Edit it and add your credentials.");
            Ok(())
        }
        ConfigSubcommand::Show => {
            // Respect --config, --profile, and CLI overrides for accurate "resolved" view.
            let config =
                atlassian_cli::Config::load_without_validation(config_path, profile, overrides)?;
            print_resolved_config(&config);
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

            if let Some(global) = atlassian_cli::Config::global_config_path() {
                let there = atlassian_cli::path_present(&global)?;
                println!("Global:  {:?} {}", global, if there { "✓" } else { "✗" });
                if there {
                    collect(&global)?;
                }
            }

            if let Some(project) = atlassian_cli::Config::project_config_path()? {
                println!("Project: {:?} ✓", project);
                collect(&project)?;
            } else {
                println!("Project: (none)");
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
            let path = if global {
                atlassian_cli::Config::global_config_path()
            } else {
                atlassian_cli::Config::project_config_path()?
                    .or_else(atlassian_cli::Config::global_config_path)
            };

            if let Some(p) = path {
                println!("{}", p.display());
            } else {
                anyhow::bail!("Config file not found");
            }
            Ok(())
        }
        ConfigSubcommand::Edit { global } => {
            let path = if global {
                atlassian_cli::Config::global_config_path()
            } else {
                atlassian_cli::Config::project_config_path()?
                    .or_else(atlassian_cli::Config::global_config_path)
            };

            let path = path.ok_or_else(|| anyhow::anyhow!("Config file not found"))?;

            if !atlassian_cli::path_present(&path)? {
                anyhow::bail!(
                    "Config file does not exist: {:?}\nRun 'atlassian-cli config init{}' to create it.",
                    path,
                    if global { " --global" } else { "" }
                );
            }

            let editor = std::env::var("EDITOR").unwrap_or_else(|_| {
                if cfg!(target_os = "macos") {
                    "open".to_string()
                } else if cfg!(target_os = "windows") {
                    "notepad".to_string()
                } else {
                    "vi".to_string()
                }
            });

            let status = std::process::Command::new(&editor).arg(&path).status()?;

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

async fn handle_jira(
    cmd: JiraCommand,
    client: &atlassian_cli::ApiClient,
) -> Result<serde_json::Value> {
    use atlassian_cli::jira;

    match cmd.subcommand {
        JiraSubcommand::Get {
            issue_key,
            fields,
            format,
        } => {
            let as_markdown = matches!(format, OutputFormat::Markdown);
            jira::get_issue(&issue_key, fields, as_markdown, client).await
        }
        JiraSubcommand::Search {
            jql,
            limit,
            all,
            stream,
            fields,
            format,
        } => {
            if stream && !all {
                anyhow::bail!("--stream requires --all flag");
            }
            let as_markdown = matches!(format, OutputFormat::Markdown);
            if all {
                jira::search_all(&jql, fields, stream, as_markdown, client).await
            } else {
                jira::search(&jql, limit, fields, as_markdown, client).await
            }
        }
        JiraSubcommand::Create {
            project,
            summary,
            issue_type,
            description,
        } => {
            let desc = description
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null);
            jira::create_issue(&project, &summary, &issue_type, desc, client).await
        }
        JiraSubcommand::Update { issue_key, fields } => {
            let fields_value: serde_json::Value = serde_json::from_str(&fields).map_err(|e| {
                anyhow::anyhow!(
                    "Invalid JSON for update fields: {}. Example: {{\"summary\":\"New title\"}}",
                    e
                )
            })?;
            jira::update_issue(&issue_key, fields_value, client).await
        }
        JiraSubcommand::Delete {
            issue_key,
            yes,
            delete_subtasks,
        } => {
            if !yes {
                anyhow::bail!(
                    "Deleting {} is irreversible (Jira has no recycle bin for issues). Re-run with --yes to confirm.",
                    issue_key
                );
            }
            jira::delete_issue(&issue_key, delete_subtasks, client).await
        }
        JiraSubcommand::Comment { action } => match action {
            CommentAction::Add { issue_key, text } => {
                jira::add_comment(&issue_key, serde_json::Value::String(text), client).await
            }
            CommentAction::Update {
                issue_key,
                comment_id,
                text,
            } => {
                jira::update_comment(
                    &issue_key,
                    &comment_id,
                    serde_json::Value::String(text),
                    client,
                )
                .await
            }
            CommentAction::List { issue_key, format } => {
                let as_markdown = matches!(format, OutputFormat::Markdown);
                jira::get_comments(&issue_key, as_markdown, client).await
            }
            CommentAction::Delete {
                issue_key,
                comment_id,
            } => jira::delete_comment(&issue_key, &comment_id, client).await,
        },
        JiraSubcommand::Transition { action } => match action {
            TransitionAction::Apply {
                issue_key,
                transition_id,
            } => jira::transition_issue(&issue_key, &transition_id, client).await,
            TransitionAction::List { issue_key } => jira::get_transitions(&issue_key, client).await,
        },
        JiraSubcommand::Link { action } => match action {
            LinkAction::Types => jira::get_link_types(client).await,
            LinkAction::Add {
                source,
                target,
                link_type,
                comment,
            } => {
                let comment_val = comment
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null);
                jira::add_link(&source, &target, &link_type, comment_val, client).await
            }
            LinkAction::Remove {
                source,
                target,
                link_type,
            } => jira::remove_link(&source, &target, link_type.as_deref(), client).await,
            LinkAction::List { issue_key } => jira::get_links(&issue_key, client).await,
        },
        JiraSubcommand::Worklog { action } => match action {
            WorklogAction::Add {
                issue_key,
                time_spent,
                comment,
                started,
            } => {
                let comment_val = comment
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null);
                jira::add_worklog(
                    &issue_key,
                    &time_spent,
                    comment_val,
                    started.as_deref(),
                    client,
                )
                .await
            }
            WorklogAction::List { issue_key } => jira::get_worklogs(&issue_key, client).await,
            WorklogAction::Update {
                issue_key,
                worklog_id,
                time_spent,
                comment,
            } => {
                let comment_val = comment
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null);
                jira::update_worklog(&issue_key, &worklog_id, &time_spent, comment_val, client)
                    .await
            }
            WorklogAction::Remove {
                issue_key,
                worklog_id,
            } => jira::remove_worklog(&issue_key, &worklog_id, client).await,
        },
        JiraSubcommand::Watcher { action } => match action {
            WatcherAction::Add { issue_key } => jira::add_watcher(&issue_key, client).await,
            WatcherAction::Remove { issue_key } => jira::remove_watcher(&issue_key, client).await,
            WatcherAction::List { issue_key } => jira::get_watchers(&issue_key, client).await,
        },
        JiraSubcommand::List { action } => match action {
            ListAction::Types => jira::get_issue_types(client).await,
            ListAction::Priorities => jira::get_priorities(client).await,
            ListAction::Statuses => jira::get_statuses(client).await,
            ListAction::Labels => jira::get_labels(client).await,
        },
        JiraSubcommand::Board { action } => match action {
            BoardAction::List { project } => jira::get_boards(&project, client).await,
        },
        JiraSubcommand::Sprint { action } => match action {
            SprintAction::List {
                board,
                project,
                state,
            } => {
                let board_id = match board {
                    Some(id) => id,
                    None => {
                        let project_key = project.ok_or_else(|| {
                            anyhow::anyhow!("Either --board or --project is required")
                        })?;
                        jira::resolve_board_id(&project_key, client).await?
                    }
                };
                jira::get_sprints(board_id, &state, client).await
            }
            SprintAction::Move { sprint_id, issues } => {
                jira::move_issues_to_sprint(sprint_id, &issues, client).await
            }
            SprintAction::Backlog { issues } => jira::move_issues_to_backlog(&issues, client).await,
        },
        JiraSubcommand::Epic { action } => match action {
            EpicAction::Assign { epic_key, issues } => {
                jira::assign_issues_to_epic(&epic_key, &issues, client).await
            }
            EpicAction::Unassign { issues } => {
                jira::unassign_issues_from_epic(&issues, client).await
            }
        },
    }
}

async fn handle_confluence(
    cmd: ConfluenceCommand,
    client: &atlassian_cli::ApiClient,
) -> Result<serde_json::Value> {
    use atlassian_cli::confluence;

    match cmd.subcommand {
        ConfluenceSubcommand::Search {
            query,
            limit,
            all,
            stream,
            expand,
            format,
        } => {
            if stream && !all {
                anyhow::bail!("--stream requires --all flag");
            }
            let as_markdown = matches!(format, OutputFormat::Markdown);
            if all {
                confluence::search_all(&query, limit, None, expand, stream, as_markdown, client)
                    .await
            } else {
                confluence::search(&query, limit, None, expand, as_markdown, client).await
            }
        }
        ConfluenceSubcommand::Get { page_id, format } => {
            let as_markdown = matches!(format, OutputFormat::Markdown);
            confluence::get_page(&page_id, None, None, as_markdown, client).await
        }
        ConfluenceSubcommand::Create {
            space,
            title,
            content,
            parent,
        } => {
            confluence::create_page(
                &space,
                &title,
                &content,
                parent.as_deref(),
                None,
                None,
                client,
            )
            .await
        }
        ConfluenceSubcommand::Update {
            page_id,
            title,
            content,
            parent,
        } => {
            confluence::update_page(
                &page_id,
                &title,
                &content,
                parent.as_deref(),
                None,
                None,
                client,
            )
            .await
        }
        ConfluenceSubcommand::Children { page_id } => {
            confluence::get_page_children(&page_id, client).await
        }
        ConfluenceSubcommand::Comment { action } => match action {
            ConfluenceCommentAction::List {
                page_id,
                location,
                roots_only,
                format,
            } => {
                let families = match location {
                    Some(location) => vec![location.into()],
                    None => confluence::CommentFamily::ALL.to_vec(),
                };
                confluence::get_comments(
                    &page_id,
                    &families,
                    !roots_only,
                    matches!(format, OutputFormat::Markdown),
                    client,
                )
                .await
            }
            ConfluenceCommentAction::Get {
                comment_id,
                location,
                format,
            } => {
                confluence::get_comment(
                    &comment_id,
                    location.into(),
                    matches!(format, OutputFormat::Markdown),
                    client,
                )
                .await
            }
            ConfluenceCommentAction::Replies {
                comment_id,
                location,
                format,
            } => {
                confluence::get_comment_replies(
                    &comment_id,
                    location.into(),
                    matches!(format, OutputFormat::Markdown),
                    client,
                )
                .await
            }
            ConfluenceCommentAction::Add {
                page_id,
                body,
                reply_to,
            } => confluence::add_comment(&page_id, &body, reply_to.as_deref(), client).await,
            ConfluenceCommentAction::Update { comment_id, body } => {
                confluence::update_comment(&comment_id, &body, client).await
            }
            ConfluenceCommentAction::Delete { comment_id } => {
                confluence::delete_comment(&comment_id, client).await
            }
        },
        ConfluenceSubcommand::Label { action } => match action {
            ConfluenceLabelAction::List { page_id } => {
                confluence::get_labels(&page_id, client).await
            }
            ConfluenceLabelAction::Add { page_id, label } => {
                confluence::add_label(&page_id, &label, client).await
            }
            ConfluenceLabelAction::Remove { page_id, label } => {
                confluence::remove_label(&page_id, &label, client).await
            }
        },
        ConfluenceSubcommand::Property { action } => match action {
            ConfluencePropertyAction::List { page_id } => {
                confluence::get_properties(&page_id, client).await
            }
            ConfluencePropertyAction::Set {
                page_id,
                key,
                value,
            } => {
                let parsed: serde_json::Value = serde_json::from_str(&value).map_err(|e| {
                    anyhow::anyhow!(
                        "value must be valid JSON (quote strings, e.g. '\"done\"'): {}",
                        e
                    )
                })?;
                confluence::set_property(&page_id, &key, parsed, client).await
            }
            ConfluencePropertyAction::Delete { page_id, key } => {
                confluence::delete_property(&page_id, &key, client).await
            }
        },
        ConfluenceSubcommand::Space { action } => match action {
            ConfluenceSpaceAction::List => confluence::get_spaces(client).await,
            ConfluenceSpaceAction::Get { space_key } => {
                confluence::get_space(&space_key, client).await
            }
        },
        ConfluenceSubcommand::Attachment { action } => match action {
            ConfluenceAttachmentAction::List { page_id } => {
                confluence::get_attachments(&page_id, client).await
            }
            ConfluenceAttachmentAction::Upload {
                page_id,
                file,
                comment,
                minor,
                content_type,
            } => {
                confluence::upload_attachment(
                    &page_id,
                    &file,
                    comment.as_deref(),
                    minor,
                    content_type.as_deref(),
                    client,
                )
                .await
            }
        },
        ConfluenceSubcommand::Delete { page_id, yes } => {
            if !yes {
                anyhow::bail!(
                    "Deleting page {} moves it to the trash. Re-run with --yes to confirm.",
                    page_id
                );
            }
            confluence::delete_page(&page_id, client).await
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

/// Print the resolved config as TOML for the active profile. Secrets are
/// masked via each `AuthConfig` variant's `display_lines`. Output is
/// copy-pasteable after replacing redactions with real secrets.
fn print_resolved_config(config: &atlassian_cli::Config) {
    let profile = &config.profile;
    println!("[{profile}]");
    match &config.domain {
        Some(d) => println!("domain = {:?}", d),
        None => println!("# domain = (not set)"),
    }

    println!();
    match &config.auth {
        Some(auth) => {
            println!("[{profile}.auth]");
            for line in auth.display_lines() {
                println!("{}", line);
            }
        }
        None => {
            println!("# [{profile}.auth] (not configured — set ATLASSIAN_AUTH_METHOD)");
        }
    }

    println!();
    println!("[{profile}.jira]");
    println!("projects_filter = {:?}", config.jira.projects_filter);
    if let Some(ref fields) = config.jira.search_default_fields {
        println!("search_default_fields = {:?}", fields);
    }
    if !config.jira.search_custom_fields.is_empty() {
        println!(
            "search_custom_fields = {:?}",
            config.jira.search_custom_fields
        );
    }

    println!();
    println!("[{profile}.confluence]");
    println!("spaces_filter = {:?}", config.confluence.spaces_filter);

    println!();
    println!("[{profile}.performance]");
    println!(
        "request_timeout_ms = {}",
        config.performance.request_timeout_ms
    );
    println!(
        "rate_limit_delay_ms = {}",
        config.performance.rate_limit_delay_ms
    );

    if let Some(ref excludes) = config.optimization.response_exclude_fields {
        println!();
        println!("[{profile}.optimization]");
        println!("response_exclude_fields = {:?}", excludes);
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
                .timeout(std::time::Duration::from_secs(60))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()?;

            let outcome = OAuthStrategy::login(params, &config.profile, &http, !no_browser).await?;

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
            let (session, unreadable) = match TokenStore::new(&config.profile)?.load().await {
                Ok(session) => (session, None),
                Err(e) => (None, Some(e)),
            };

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
                    // Under the opt-out the keychain was never asked, so "not
                    // logged in" would be a claim about a place this run did
                    // not look — and a session stored there before the flag is
                    // exactly what `auth login` would shadow rather than reach.
                    None if atlassian_cli::auth::keychain_opt_out() => println!(
                        "No session in the file store (profile: {}). ATLASSIAN_NO_KEYCHAIN is \
                         set, so the keychain was not consulted; unset it for one run to see \
                         or clear what it holds.",
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
            let strategy = OAuthStrategy::resume(params, &config.profile).await?;
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
mod tests {
    use super::*;
    use atlassian_cli::dist::Installation;
    // Only the `#[cfg(unix)]` tests below deploy a skill.
    #[cfg(unix)]
    use atlassian_cli::dist::skill;

    /// The binary is the only thing that knows where the skill and the config
    /// are. Where the home directory cannot be found, skipping them silently
    /// and removing it anyway leaves them with nothing to find them by.
    #[cfg(unix)]
    #[tokio::test]
    async fn an_unknown_home_refuses_before_anything_goes() {
        let home = tempfile::tempdir().unwrap();
        let binary = home.path().join("bin").join("atlassian-cli");
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        std::fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
        let installation = Installation::at(binary.clone(), None);

        let err = self_uninstall(&installation, false, true, true)
            .await
            .unwrap_err()
            .to_string();

        assert!(err.contains("home directory"), "{err}");
        assert!(binary.is_file(), "the binary went with the home unknown");
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

        let report = self_uninstall(&installation, false, true, false)
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

        let report = self_uninstall(&installation, true, true, false)
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

        let report = self_uninstall(&installation, true, true, true)
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

        let report = self_uninstall(&installation, true, true, true)
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

        let report = self_uninstall(&installation, true, true, true)
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

        self_uninstall(&installation, true, true, true)
            .await
            .unwrap();

        assert!(stray.is_file());
    }

    /// Each step is irreversible, so a failure has to name what it already did.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_binary_that_cannot_be_removed_reports_what_already_went() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let installation = installation(home.path());
        let skill_dir = installation.skill_dir().unwrap();
        skill::deploy(&skill_dir).unwrap();

        let bin_dir = installation.binary().parent().unwrap().to_path_buf();
        std::fs::set_permissions(&bin_dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let err = self_uninstall(&installation, false, true, false)
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

        let report = self_uninstall(&installation, true, false, false)
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

        let report = self_uninstall(&installation, false, true, false)
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

        let err = self_uninstall(&installation, true, false, false)
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
        use std::os::unix::fs::PermissionsExt;
        mock_keychain();
        let home = tempfile::tempdir().unwrap();
        let installation = installation(home.path());
        write_credentials(&installation, &["default"]);
        let dir = installation.config_dir().unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000)).unwrap();

        let outcome = self_uninstall(&installation, true, false, false).await;
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
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let installation = installation(home.path());
        let dir = installation.skill_dir().unwrap();
        skill::deploy(&dir).unwrap();
        let parent = dir.parent().unwrap().to_path_buf();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o000)).unwrap();

        let outcome = self_uninstall(&installation, false, true, false).await;
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

        let report = self_uninstall(&installation, true, true, false)
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
        let store = atlassian_cli::auth::TokenStore::at("logout-corrupt", file);

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
