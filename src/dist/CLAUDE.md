# dist module

Backs the `self` command group: what this installation is, replacing its binary, and the skill it deploys. Talks to GitHub, not Atlassian, so it shares nothing with `ApiClient`.

Everything runs in-process — `reqwest` for the download, `sha2` for the checksum, `flate2`/`tar` for the archive. Nothing is shelled out to, so an update works wherever the binary does. The one exception is `gh attestation verify`, which is opt-in and is the command a person would run by hand to answer the same question.

## The skill is compiled in

`skill.rs` carries `SKILL.md` through `include_str!`. There is one artifact, so a deployed copy cannot be a different version from the binary — and `SkillState` is decided by **byte comparison**, not a version string, so an edited copy is detected too. Adding a file to the skill is a line in `FILES`; editing one rebuilds the binary that carries it.

`deploy` also removes the files in the skill directory that this binary does not carry, so the directory states this version and nothing else. That set is deliberately narrow, because it is a set the code *deletes*: regular files, one level deep, classified with `symlink_metadata`, and named by `OsString` — a lossy `String` does not name the file it came from, so pruning one would fail on a name the platform allows and take every deploy with it. Following a symlink resolves the deletion somewhere it was never pointed — a link inside the directory, or a skill directory the user redirected into a dotfiles repository. For a directory this tool does not reconcile, `reconcilable_files` answers `None` rather than an empty set, and the distinction is load-bearing in both directions: nothing to prune, and nothing there that `state` counts as a difference. An empty set would make a redirected directory read `stale` forever, immediately after a deploy that had just written it correctly. `FILES` must stay flat; a test enforces it, along with `SKILL.md`'s `version:` equalling `CARGO_PKG_VERSION`.

Writing has the same reach as deleting and needs the same guard: `fs::write` opens *through* a symlink, so a deploy unlinks a carried name that **is** a link before writing it. Without that, a `SKILL.md` pointed at `~/.zshrc` had that file's contents replaced with the skill. Only a link — an ordinary file stays a truncating write, which needs permission on the file rather than on the directory, so a deploy still lands in a skill directory the user made read-only.

`remove` takes the skill directory and an emptied `~/.claude/skills`, never `~/.claude` — that one holds the agent's own state. It classifies with `symlink_metadata` rather than `exists`, which reports a dangling link as nothing there and leaves it behind.

Deliberately absent: fetching the skill from a git ref, and detecting a source checkout to prefer. Both existed in the shell installer and were the mechanism by which the skill and binary drifted.

## Update order: verify before replacing

`self update` runs download → checksum → extract → **run the staged binary and compare the version it prints** → replace. The version check happens before anything is touched, so a download that will not run on this machine (wrong architecture, failed code signing, truncated archive) ends with the installation untouched.

There is therefore no rollback path, and that is the point: an installation whose binary does not run has no way left to repair itself, so the design guarantees that state is never created rather than trying to recover from it. What the ordering does not make atomic is the replacement itself — on Windows `self_replace` renames the running executable aside before copying the new one in, so an I/O failure between those steps leaves the install path empty. That is the dependency's, not this ordering's, and it is why the version check happens while there is still something to keep.

`fetch_verified_binary` covers download through extraction and is driven by wiremock tests; only the final `install` touches the running binary.

## Which release is latest

`resolve_latest` asks the GitHub API first and falls back to the `/releases/latest` web redirect. They disagree in one direction: the web view trails the API by minutes after a publish, which is exactly when someone runs an update — read in that window it names the previous release and the update calls the running binary current. So the answer carries its `Provenance` and callers report it.

`decide` is pure — no I/O — and refuses a downgrade that the release channel offered. A yanked release and a stale answer from the trailing source have the same shape, so going back takes an explicit `--version`.

## Archives are read, never unpacked

`read_from_tar_gz` returns one named member's bytes. No path from the archive reaches the filesystem, so archive-directed writes are not a thing that can happen.

Every target publishes a `.tar.gz`, Windows included, so there is one extraction path. `target.rs`'s table must match the `Release` workflow's build matrix or a download 404s; a test reads `release.yml` and holds them together.

## Paths come from their owners

`layout.rs` composes: the directory from `Config::global_config_dir_in`, the file names from `Config::GLOBAL_CONFIG_FILE` and `auth::CREDENTIALS_FILE`. It does not reassemble `~/.config/atlassian-cli` — a second derivation is how a command ends up reporting on one directory while another writes a different one. Only the skill directory is owned here.

Every path hangs off the one `home` the `Installation` holds, which is also what makes `Installation::at` useful: the uninstall steps are exercised against a temporary home in `src/main.rs`'s tests rather than read for correctness.

## Uninstall enumerates rather than guesses

`auth::stored_profiles` lists what the keychain holds under this tool's service, in the spelling that store's `search` takes — see `src/auth/CLAUDE.md`, and note that "listed" is a claim about the entries this tool could name, not about every entry there is. Which answers refuse is decided by whether a token could still be there afterwards:

| enumeration | meaning | uninstall |
|---|---|---|
| `Listed` | the entries are known | proceeds |
| `Unsupported` | this build carries no store, so nothing was ever written to one | proceeds — nothing to miss |
| `Skipped` | `ATLASSIAN_NO_KEYCHAIN` forbids the look, and a session from before the flag may be in there | refuses |
| `Failed` | a store that exists and would not answer — locked, or a session bus out of reach | refuses |

Every target in `target.rs` compiles a store in, so `Unsupported` cannot arise in a released binary; a machine whose keychain is out of reach reports `Failed` and refuses. The row exists for a source build on a platform none of the store crates cover.

The line between the middle two rows is drawn by the build, not by the failure. Of the three, only the Secret Service store's constructor talks to anything — it connects to the session bus, so it is the one that can fail where a store exists, and that failure says nothing about what is in the keyring. Reading it as absence is how a token saved from a desktop session survives an uninstall run over SSH.

A refusal comes before the skill, the config and the binary, because removing the binary takes away the only thing that knows where those tokens are. So a box whose keychain never answers finishes in two runs: the first clears the credentials file and refuses, naming what it already removed, and `--keep-credentials` completes the second.

The credentials file is cleared first regardless — it belongs to this installation, and a keychain that cannot be reached is no reason to leave it. `TokenStore::delete` clears both backends independently for the same reason: the machines where the keychain refuses are the ones that keep tokens in the file. It goes through `auth::remove_credentials_file`, which requires the path to be a regular file: unlinking a symlink clears the name and leaves every token readable at the far end.

Nothing here uses `Path::exists` to decide whether something is there; `present` does. `exists` answers false to two different questions — nothing there, and could not tell — and it resolves a link, so a dangling one reads as absent too. Every step below is irreversible and the last one takes the binary that knows where the rest is, so a step skipped on either reading leaves something behind and calls it removed. Absence is concluded from `NotFound` alone.

Paths reach a report through `display_path`. `json!` serializes a `Path` by unwrapping, so a path the platform allows and UTF-8 cannot spell would answer with a panic instead of the single-line error object the CLI contract promises.

`--purge-config` removes the config file this tool writes and then the directory only if that leaves it empty; a directory that survives is reported as kept rather than passed over in silence.

The binary goes last and through `self_replace::self_delete_at`: Windows refuses to unlink a running executable, so a plain `remove_file` would fail there after everything else had already gone. A failure at that point names what was already removed, because "Permission denied" alone reads as "nothing happened".
