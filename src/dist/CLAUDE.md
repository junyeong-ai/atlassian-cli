# dist module

Backs the `self` command group: what this installation is, replacing its binary, and the skill it deploys. Talks to GitHub, not Atlassian, so it shares nothing with `ApiClient`.

Everything runs in-process — `reqwest` for the download, `sha2` for the checksum, `flate2`/`tar` for the archive. Nothing is shelled out to, so an update works wherever the binary does. The one exception is `gh attestation verify`, which is opt-in and is the command a person would run by hand to answer the same question.

## The skill is compiled in

`skill.rs` carries `SKILL.md` through `include_str!`. There is one artifact, so a deployed copy cannot be a different version from the binary — and `SkillState` is decided by **byte comparison**, not a version string, so an edited copy is detected too. Adding a file to the skill is a line in `FILES`; editing one rebuilds the binary that carries it.

`deploy` also removes the files in the skill directory that this binary does not carry, so the directory states this version and nothing else. That set is deliberately narrow, because it is a set the code *deletes*: regular files, one level deep, classified with `symlink_metadata`, and empty for a directory that is itself a symlink. Following any of those resolves the deletion somewhere it was never pointed — a link inside the directory, or a skill directory the user redirected into a dotfiles repository. `FILES` must therefore stay flat; a test enforces it, along with `SKILL.md`'s `version:` equalling `CARGO_PKG_VERSION`.

`remove` takes the skill directory and an emptied `~/.claude/skills`, never `~/.claude` — that one holds the agent's own state.

Deliberately absent: fetching the skill from a git ref, and detecting a source checkout to prefer. Both existed in the shell installer and were the mechanism by which the skill and binary drifted.

## Update order: verify before replacing

`self update` runs download → checksum → extract → **run the staged binary and compare the version it prints** → replace. The version check happens before anything is touched, so a download that will not run on this machine (wrong architecture, failed code signing, truncated archive) ends with the installation untouched.

There is therefore no rollback path, and that is the point: an installation whose binary does not run has no way left to repair itself, so the design guarantees that state is never created rather than trying to recover from it.

`fetch_verified_binary` covers download through extraction and is driven by wiremock tests; only the final `install` touches the running binary.

## Which release is latest

`resolve_latest` asks the GitHub API first and falls back to the `/releases/latest` web redirect. They disagree in one direction: the web view trails the API by minutes after a publish, which is exactly when someone runs an update — read in that window it names the previous release and the update calls the running binary current. So the answer carries its `Provenance` and callers report it.

`decide` is pure — no I/O — and refuses a downgrade that the release channel offered. A yanked release and a stale answer from the trailing source have the same shape, so going back takes an explicit `--version`.

## Archives are read, never unpacked

`read_from_tar_gz` returns one named member's bytes. No path from the archive reaches the filesystem, so archive-directed writes are not a thing that can happen.

Every target publishes a `.tar.gz`, Windows included, so there is one extraction path. `target.rs`'s table must match the `Release` workflow's build matrix or a download 404s; a test reads `release.yml` and holds them together.

## Paths come from their owners

`layout.rs` calls `Config::global_config_dir()` and `auth::credentials_file()`. It does not reassemble `~/.config/atlassian-cli` — a second derivation is how a command ends up reporting on one directory while another writes a different one. Only the skill directory is owned here.

## Uninstall enumerates rather than guesses

Every platform store `keyring-core` links (macOS Keychain, Windows Credential Manager, Secret Service) implements `search`, so `auth::stored_profiles` lists the entries under the `atlassian-cli` service and that is the complete set. An enumeration that did not happen — a store that cannot search, or `ATLASSIAN_NO_KEYCHAIN` forbidding the look — **refuses the uninstall before anything is removed**. The step after clearing tokens deletes the binary, so proceeding would leave tokens behind along with nothing that knows where they are. `TokenStore::delete` propagates a keychain that refused for the same reason; only "nothing there" and "no keychain in play" count as success.

The credentials file is removed because it belongs to this installation, not because its contents parsed; a corrupt one still holds tokens.

`--purge-config` removes the config file this tool writes and then the directory only if that leaves it empty. `credentials.json` lives in that directory, so a `remove_dir_all` there would take it whatever `--keep-credentials` said — and would take anything else the user keeps beside it.

The binary goes last and through `self_replace::self_delete_at`: Windows refuses to unlink a running executable, so a plain `remove_file` would fail there after everything else had already gone. A failure at that point names what was already removed, because "Permission denied" alone reads as "nothing happened".
