# dist module

Backs the `self` command group: what this installation is, replacing its binary, and the skill it deploys. Talks to GitHub, not Atlassian, so it shares nothing with `ApiClient`.

Everything runs in-process — `reqwest` for the download, `sha2` for the checksum, `flate2`/`tar` for the archive. Nothing is shelled out to, so an update works wherever the binary does. The one exception is `gh attestation verify`, which is opt-in and is the command a person would run by hand to answer the same question.

## The skill is compiled in

`skill.rs` carries `SKILL.md` through `include_str!`. There is one artifact, so a deployed copy cannot be a different version from the binary — and `SkillState` is decided by **byte comparison**, not a version string, so an edited copy is detected too. Adding a file to the skill is a line in `FILES`; editing one rebuilds the binary that carries it.

`deploy` also removes anything in the skill directory that this binary does not carry, so the directory states this version and nothing else. A test asserts `SKILL.md`'s `version:` equals `CARGO_PKG_VERSION` — bump both together.

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

Every platform store `keyring-core` links (macOS Keychain, Windows Credential Manager, Secret Service) implements `search`, so `auth::stored_profiles` lists the entries under the `atlassian-cli` service and that is the complete set. Where a store still answers `NotSupportedByStore`, `KeyringEnumeration` reports it instead of the caller claiming completeness — telling someone their credentials are gone when they are not is the failure to avoid.

The credentials file is removed because it belongs to this installation, not because its contents parsed; a corrupt one still holds tokens.
