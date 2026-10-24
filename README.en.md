# Atlassian CLI

[![CI](https://github.com/junyeong-ai/atlassian-cli/workflows/CI/badge.svg)](https://github.com/junyeong-ai/atlassian-cli/actions/workflows/ci.yml)
[![Security](https://github.com/junyeong-ai/atlassian-cli/workflows/Security/badge.svg)](https://github.com/junyeong-ai/atlassian-cli/actions/workflows/security.yml)
[![Rust](https://img.shields.io/badge/rust-1.98.0%2B%20(2024%20edition)-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Version](https://img.shields.io/badge/version-0.14.0-blue?style=flat-square)](https://github.com/junyeong-ai/atlassian-cli/releases)
[![DeepWiki](https://img.shields.io/badge/DeepWiki-junyeong--ai%2Fatlassian--cli-blue.svg?logo=data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACwAAAAyCAYAAAAnWDnqAAAAAXNSR0IArs4c6QAAA05JREFUaEPtmUtyEzEQhtWTQyQLHNak2AB7ZnyXZMEjXMGeK/AIi+QuHrMnbChYY7MIh8g01fJoopFb0uhhEqqcbWTp06/uv1saEDv4O3n3dV60RfP947Mm9/SQc0ICFQgzfc4CYZoTPAswgSJCCUJUnAAoRHOAUOcATwbmVLWdGoH//PB8mnKqScAhsD0kYP3j/Yt5LPQe2KvcXmGvRHcDnpxfL2zOYJ1mFwrryWTz0advv1Ut4CJgf5uhDuDj5eUcAUoahrdY/56ebRWeraTjMt/00Sh3UDtjgHtQNHwcRGOC98BJEAEymycmYcWwOprTgcB6VZ5JK5TAJ+fXGLBm3FDAmn6oPPjR4rKCAoJCal2eAiQp2x0vxTPB3ALO2CRkwmDy5WohzBDwSEFKRwPbknEggCPB/imwrycgxX2NzoMCHhPkDwqYMr9tRcP5qNrMZHkVnOjRMWwLCcr8ohBVb1OMjxLwGCvjTikrsBOiA6fNyCrm8V1rP93iVPpwaE+gO0SsWmPiXB+jikdf6SizrT5qKasx5j8ABbHpFTx+vFXp9EnYQmLx02h1QTTrl6eDqxLnGjporxl3NL3agEvXdT0WmEost648sQOYAeJS9Q7bfUVoMGnjo4AZdUMQku50McDcMWcBPvr0SzbTAFDfvJqwLzgxwATnCgnp4wDl6Aa+Ax283gghmj+vj7feE2KBBRMW3FzOpLOADl0Isb5587h/U4gGvkt5v60Z1VLG8BhYjbzRwyQZemwAd6cCR5/XFWLYZRIMpX39AR0tjaGGiGzLVyhse5C9RKC6ai42ppWPKiBagOvaYk8lO7DajerabOZP46Lby5wKjw1HCRx7p9sVMOWGzb/vA1hwiWc6jm3MvQDTogQkiqIhJV0nBQBTU+3okKCFDy9WwferkHjtxib7t3xIUQtHxnIwtx4mpg26/HfwVNVDb4oI9RHmx5WGelRVlrtiw43zboCLaxv46AZeB3IlTkwouebTr1y2NjSpHz68WNFjHvupy3q8TFn3Hos2IAk4Ju5dCo8B3wP7VPr/FGaKiG+T+v+TQqIrOqMTL1VdWV1DdmcbO8KXBz6esmYWYKPwDL5b5FA1a0hwapHiom0r/cKaoqr+27/XcrS5UwSMbQAAAABJRU5ErkJggg==)](https://deepwiki.com/junyeong-ai/atlassian-cli)

> **🌐 [한국어](README.md)** | **English**

A single Rust binary for Jira and Confluence Cloud, built to be driven by AI agents as well as people. Jira and Confluence commands print JSON, rich text is read and written as Markdown, long bodies come from standard input, and a failure is one JSON object with a stable exit code. It ships the `jira-confluence` Claude Code skill and supports four authentication methods: OAuth 2.0 (3LO), service accounts, scoped API tokens and classic API tokens.

---

## Quick start

```bash
# 1. Install
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | bash
export PATH="${INSTALL_DIR:-$HOME/.local/bin}:$PATH"

# 2. Create a config and pick an authentication method (see Authentication)
atlassian-cli config init --global
atlassian-cli config edit --global

# 3. Sign in (oauth only; add --no-browser on a machine without one)
atlassian-cli auth login

# 4. Check the credentials and run a first search
atlassian-cli config validate
atlassian-cli jira search "assignee = currentUser()" --limit 5
```

---

## Usage

### Jira

```bash
# Read
atlassian-cli jira get PROJ-123 --format markdown
atlassian-cli jira search "assignee = currentUser() AND statusCategory != Done" --format markdown --limit 20
atlassian-cli jira search "project = PROJ" --all --stream > issues.jsonl
atlassian-cli jira comment list PROJ-123 --format markdown

# Write — Markdown becomes ADF, and `-` reads the body from standard input
atlassian-cli jira create PROJ "Login fails on Safari" Bug --format markdown --description - < report.md
atlassian-cli jira comment add PROJ-123 "Reproduced on **staging**" --format markdown
atlassian-cli jira user search "Kim"                  # accountId for assignee fields, JQL and CQL
atlassian-cli jira update PROJ-123 '{"assignee": {"accountId": "5b10a2844c20165700ede21g"}}'
atlassian-cli jira transition list PROJ-123
atlassian-cli jira transition apply PROJ-123 31
atlassian-cli jira delete PROJ-123 --yes               # permanent

# Links, worklogs, watchers and agile
atlassian-cli jira link add PROJ-1 PROJ-2 --type Blocks
atlassian-cli jira worklog add PROJ-123 "2h 30m" --comment "Investigation"
atlassian-cli jira watcher add PROJ-123
atlassian-cli jira sprint list --project PROJ
atlassian-cli jira sprint move 55 PROJ-1 PROJ-2
atlassian-cli jira epic assign EPIC-1 PROJ-1 PROJ-2    # sets each issue's parent
```

### Confluence

```bash
# Read
atlassian-cli confluence search "space = TEAM AND type = page" --format markdown --limit 10
atlassian-cli confluence get 123456 --format markdown
atlassian-cli confluence children 123456               # pages, folders, whiteboards, databases, embeds
atlassian-cli confluence comment list 123456 --format markdown

# Write — Markdown becomes storage format
atlassian-cli confluence create TEAM "Design notes" - --format markdown --parent 123456 < notes.md
atlassian-cli confluence update 123456 "Design notes" - --format markdown --base-version 7 < notes.md
atlassian-cli confluence comment add 123456 "Looks **good**" --format markdown
atlassian-cli confluence attachment upload 123456 ./diagram.png
atlassian-cli confluence label add 123456 needs-review
atlassian-cli confluence property set 123456 review '{"status":"done"}'
atlassian-cli confluence delete 123456 --yes           # moves the page to the trash
```

`--base-version` is the `version.number` you read. When someone saved after that version, the update is refused with 409 instead of overwriting their change.

### Output for scripts and agents

- Results of `jira`, `confluence` and `self` go to stdout as JSON, and lists use an `items` envelope. Root options such as `--pretty`, `--profile` and `--config` precede the subcommand.
- `--format` names the rich-text representation: `adf` for Jira or `storage` for Confluence by default, or `markdown`, which converts read content inside the JSON envelope and write bodies before they are sent. `html` remains an alias of the default.
- Body arguments take `-` to read standard input, and refuse it empty. A heredoc with a quoted delimiter (`<<'EOF'`) needs no escaping and has no argument-size limit.
- `--all` follows every page of a search; `--all --stream` writes JSONL as pages arrive and reports progress on stderr.
- Markdown writes follow CommonMark with GitHub tables and strikethrough. ADF cannot hold a heading, quote, table or rule inside a list item or quote, so such Markdown is refused with its location. Raw HTML stays literal text; write Confluence macros with `--format storage`.

The generated [CLI reference](docs/cli.md) lists every command, argument and default; `atlassian-cli <command> --help` describes the installed version.

```bash
atlassian-cli completions zsh > "${fpath[1]}/_atlassian-cli"   # also bash, fish, elvish, powershell
```

---

## Installation

```bash
# Latest release; deploys the bundled skill to ~/.claude/skills
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | bash

# A specific release, or another directory
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | ATLASSIAN_CLI_VERSION=v0.14.0 bash
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | INSTALL_DIR="$HOME/bin" bash
```

**Manual install.** Download your platform's archive and its `.sha256` from [Releases](https://github.com/junyeong-ai/atlassian-cli/releases), verify it with `sha256sum -c` (Linux) or `shasum -a 256 -c` (macOS), extract it with `tar -xzf`, and move `atlassian-cli` into a directory on your `PATH`. On Windows, extract `atlassian-cli.exe` from the release `.zip`. Run `atlassian-cli self skill install` to deploy the skill.

**From source.** Building needs Rust 1.98.0 or later; `rust-toolchain.toml` pins the toolchain the project is developed with.

```bash
git clone https://github.com/junyeong-ai/atlassian-cli
cd atlassian-cli
cargo install --path . --locked --root "$HOME/.local" --target "$(rustc --print host-tuple)"
```

Add `--force` to replace a binary installed another way.

Releases cover Linux (x86_64, aarch64), macOS (Intel, Apple Silicon) and Windows (x86_64). `install.sh` runs on Linux and macOS. Every platform publishes a `.tar.gz`, so `self update` works everywhere.

### Managing the installation

```bash
atlassian-cli self status                          # version, paths, skill state, profiles holding tokens (no network)
atlassian-cli self update                          # replace with the latest release
atlassian-cli self update --version 0.13.9         # a specific release; going back needs the version given
atlassian-cli self update --verify-attestations    # also check GitHub build provenance (needs the gh CLI)
atlassian-cli self update --force                  # replace even a binary that already reports that version
atlassian-cli self skill install                   # redeploy the skill this binary carries
atlassian-cli self skill remove --yes
atlassian-cli self uninstall --yes [--keep-skill] [--keep-credentials] [--purge-config]
```

- `self update` runs the downloaded binary and checks the version it reports before replacing anything, so a binary that cannot run on this machine leaves the installation untouched.
- The skill is compiled into the binary. `self status` compares the deployed copy byte for byte and reports `stale` when it differs, as it does after an edit or a downgrade.
- `self uninstall` also clears OAuth tokens from the OS keychain unless `--keep-credentials` is given. It refuses when a keychain exists but cannot be read, because tokens left behind with no tool that knows about them are the worst outcome. When the keychain does not answer, the first run clears `credentials.json` and refuses, naming what it removed, and a second run with `--keep-credentials` finishes.
- `auth logout` exits non-zero against a keychain that does not answer, for the same reason. Under `ATLASSIAN_NO_KEYCHAIN` it exits zero and says the keychain was not touched.
- `--purge-config` removes the config file this tool wrote, and its directory only when that leaves it empty. Project-local `.atlassian.toml` files are never touched.

---

## Authentication

Choose a method explicitly; it is never inferred.

| Method | Acts as | Request host | Notes |
|---|---|---|---|
| `oauth` ⭐ | the signed-in user | `api.atlassian.com` | browser sign-in; tokens kept in the OS keychain and refreshed |
| `service_account` | a service account | `api.atlassian.com` | OAuth 2.0 client credentials for automation and CI |
| `scoped_token` | the token's owner | `api.atlassian.com` | API token **with** scopes; limited to those scopes |
| `basic` | the token's owner | `{domain}` | classic API token **without** scopes |

The two token methods do not accept each other's tokens: the site host answers 401 to a token carrying scopes, and the gateway rejects a classic one. A 401 carries a `hint` naming the causes to check, such as expiry, the wrong method or a missing scope. Create either kind of token at <https://id.atlassian.com/manage-profile/security/api-tokens>; Atlassian recommends scoped tokens, and tokens issued before 2024-12-15 expired between 2026-03-14 and 2026-05-12 ([token management](https://support.atlassian.com/atlassian-account/docs/manage-api-tokens-for-your-atlassian-account/)).

### OAuth 2.0 (3LO) — recommended

```toml
# ~/.config/atlassian-cli/config.toml
[default.auth]
method = "oauth"
client_id = "..."          # from developer.atlassian.com
client_secret = "..."      # prefer the ATLASSIAN_CLIENT_SECRET environment variable
redirect_port = 8976       # must match the app's callback URL
# scopes defaults to ["read:jira-user", "read:jira-work", "write:jira-work", "offline_access"]
# cloud_id = "..."         # pin one site when the account reaches several
```

Set up the app at <https://developer.atlassian.com/console/myapps/>: create an OAuth 2.0 (3LO) app, add `http://127.0.0.1:8976/callback` as its callback URL, grant the scopes the profile requests, and copy the client ID and secret. A scope the app does not grant is refused at consent.

```bash
atlassian-cli auth login       # opens the browser; --no-browser prints the URL instead
atlassian-cli auth status      # expiry, scopes, storage backend
atlassian-cli auth refresh     # force a refresh
atlassian-cli auth logout      # clear the stored session
```

`auth login` waits up to five minutes for the browser sign-in. `--no-browser` only prints the URL: the browser that signs in must still reach `127.0.0.1:8976` (the profile's `redirect_port`) on the machine running the CLI. On a remote host reached over SSH, forward the port with `ssh -L 8976:127.0.0.1:8976 <host>` before signing in.

Sessions with a refresh token refresh five minutes before expiry. Beyond the default Jira grants, add to the profile only the scopes your commands use:

| Command | Additional scopes | API |
|---|---|---|
| `board list`, `sprint list --project` | `read:board-scope:jira-software`, `read:project:jira` | [Board](https://developer.atlassian.com/cloud/jira/software/rest/api-group-board/) |
| `sprint list` | `read:sprint:jira-software` | [Board sprints](https://developer.atlassian.com/cloud/jira/software/rest/api-group-board/#api-rest-agile-1-0-board-boardid-sprint-get) |
| `sprint move` | `write:sprint:jira-software` | [Sprint](https://developer.atlassian.com/cloud/jira/software/rest/api-group-sprint/) |
| `sprint backlog` | `write:board-scope:jira-software` | [Backlog](https://developer.atlassian.com/cloud/jira/software/rest/api-group-backlog/) |
| `confluence search` | `search:confluence`, `read:confluence-content.all` | [Search](https://developer.atlassian.com/cloud/confluence/rest/v1/api-group-search/) |
| `confluence get`, `confluence label list`, `confluence property list` | `read:page:confluence` | [Page](https://developer.atlassian.com/cloud/confluence/rest/v2/api-group-page/) |
| `confluence comment` reads | `read:comment:confluence` | [Comment](https://developer.atlassian.com/cloud/confluence/rest/v2/api-group-comment/) |
| `confluence attachment list` | `read:attachment:confluence` | [Attachment](https://developer.atlassian.com/cloud/confluence/rest/v2/api-group-attachment/) |
| `confluence space` | `read:space:confluence` | [Space](https://developer.atlassian.com/cloud/confluence/rest/v2/api-group-space/) |
| `confluence children` | `read:hierarchical-content:confluence` | [Children](https://developer.atlassian.com/cloud/confluence/rest/v2/api-group-children/) |

Confluence writes need the scopes of every endpoint they call, including a read before the write: property changes need `read:page:confluence` and `write:page:confluence`, and comment deletion needs `delete:comment:confluence`. Label writes and uploads go through the v1 API and need `write:confluence-content` and `write:confluence-file`. After enabling a scope on the app and adding it to the profile, run `auth login` again; an existing token does not gain it.

> **Headless and AI agents:** on a desktop OS the keychain can block on a GUI prompt. Set `ATLASSIAN_NO_KEYCHAIN=1` for the environment to use the 0600 file store instead, and keep it set; while it is set, `auth logout` clears only the file store. If you have signed in with the keychain, run `auth logout` once before setting it; a session left in the keychain is used again whenever the flag is unset.

### Service account

```bash
export ATLASSIAN_AUTH_METHOD=service_account
export ATLASSIAN_CLIENT_ID="..."
export ATLASSIAN_CLIENT_SECRET="..."
# ATLASSIAN_CLOUD_ID is needed only when the credential reaches more than one site
```

```toml
[default.auth]
method = "service_account"
client_id = "..."
client_secret = "..."
```

### Scoped API token

```bash
export ATLASSIAN_AUTH_METHOD=scoped_token
export ATLASSIAN_DOMAIN="company.atlassian.net"   # resolves the cloud ID
export ATLASSIAN_EMAIL="user@example.com"
export ATLASSIAN_API_TOKEN="..."
```

The cloud ID is read from `https://{domain}/_edge/tenant_info`, which needs no credentials and is this method's only request to the site host. Set `ATLASSIAN_CLOUD_ID` or `[auth].cloud_id` to skip it; the domain is then unnecessary. `config validate` checks the identity through Jira, so a token with only Confluence scopes fails it while still working: check such a token with `confluence space list`.

### Classic API token (`basic`)

```toml
[default]
domain = "company.atlassian.net"

[default.auth]
method = "basic"
email = "user@example.com"
token = "..."
```

The same values can come from `ATLASSIAN_AUTH_METHOD=basic`, `ATLASSIAN_DOMAIN`, `ATLASSIAN_EMAIL` and `ATLASSIAN_API_TOKEN`.

---

## Configuration

### Files and profiles

Configuration is read from, highest priority first: the `--config <file>` path, a project file (`.atlassian.toml` or `.atlassian/config.toml`, found by walking up from the working directory), and the global `~/.config/atlassian-cli/config.toml`. `config init --global` writes a commented starter with all four methods.

```toml
[default.auth]
method = "oauth"
client_id = "..."

[work]
domain = "work.atlassian.net"

[work.auth]
method = "basic"
email = "user@example.com"
token = "..."
```

Select a profile with `atlassian-cli --profile work ...`; separate profiles when the account, site or permissions differ.

### Precedence

Settings resolve as CLI flags > environment variables > `--config` file > project file > global file. An omitted setting inherits from the next file, and an explicit `[]` clears an inherited list. A blank environment variable counts as unset.

For the selected profile, the highest-priority file that defines an `[auth]` section supplies the whole section: omitting the section keeps authentication from a lower file, but a credential omitted inside it is not inherited. Each credential then resolves as CLI flag > environment variable > that section. Choosing another method through `ATLASSIAN_AUTH_METHOD` discards the file's credentials, so supply the new method's values through flags or environment variables.

### Managing configuration

- `config path` and `config edit` target the `--config` file when given, and the global file with `--global`.
- `atlassian-cli --config <file> config init` writes a starter there and never overwrites an existing file.
- `config list` shows the files read, including an explicit one, and which environment variables are set.
- `config show` prints the resolved profile as TOML with secrets masked.
- `EDITOR` is an executable followed by arguments in POSIX quoting on every platform, for example `code --wait`. Quote a path containing spaces or backslashes inside the value: `'C:\Program Files\Microsoft VS Code\Code.exe' --wait`.

### Scope filters and response size

```toml
[default.jira]
projects_filter = ["PROJ1", "PROJ2"]       # injected into JQL that names no project
search_default_fields = ["key", "summary", "status"]
search_custom_fields = ["customfield_10015"]

[default.confluence]
spaces_filter = ["TEAM"]                    # injected into CQL that names no space

[default.performance]
request_timeout_ms = 30000
rate_limit_delay_ms = 200

[default.optimization]
response_exclude_fields = ["self", "avatarUrls", "iconUrl"]
```

With `projects_filter`, `status = Open` runs as `project IN ("PROJ1","PROJ2") AND (status = Open)`, while a query that already names a project runs unchanged. Without `--fields`, `jira search` requests a default set of 17 fields: `search_default_fields` replaces it, and `search_custom_fields` extends it. The environment variables `JIRA_PROJECTS_FILTER`, `CONFLUENCE_SPACES_FILTER`, `JIRA_SEARCH_DEFAULT_FIELDS`, `JIRA_SEARCH_CUSTOM_FIELDS`, `RESPONSE_EXCLUDE_FIELDS` and `REQUEST_TIMEOUT_MS` override these settings.

---

## Errors and exit codes

A runtime failure prints one JSON object to stderr; stdout carries results only. Usage errors, such as an unknown option, print text and exit with `2`. A command can fail after printing output (`--stream` writes pages as they arrive), so read the exit code rather than the presence of output.

```json
{"error":{"message":"Failed to get issue (404 Not Found): ...","operation":"get issue","status":404}}
```

API failures carry `status` and `operation`, and a `hint` when a remediation is known; a 429 also carries `retryAfterSeconds` and `rateLimitReason` when Atlassian sends them. Reads and idempotent requests retry a 429 up to three times, waiting at least as long as the server asks, and a wait longer than 60 seconds is returned to the caller rather than slept through. Any other write may already have been applied, so it returns at once; check whether it took effect before sending it again. An upload is not retried because its body cannot be resent, and running it again adds a new version of the same file. The policies are in Atlassian's rate limiting guides for [Jira](https://developer.atlassian.com/cloud/jira/platform/rate-limiting/) and [Confluence](https://developer.atlassian.com/cloud/confluence/rate-limiting/).

| Exit code | Meaning |
|---|---|
| `0` | success |
| `1` | generic failure |
| `2` | usage error |
| `3` | authentication or permission (401, 403) |
| `4` | not found (404) |
| `5` | rate limited (429) |
| `6` | server error (5xx) |

---

## Troubleshooting

- **401 with `basic` or `scoped_token`** — the token expired, or its shape does not match the method: a token with scopes needs `scoped_token`, a classic one needs `basic`. Read the `hint` before replacing credentials.
- **401 "scope does not match"** — the OAuth token lacks a scope this endpoint needs; other commands still work. Grant the scope, add it to the profile and run `auth login` again.
- **403 for API tokens** — an administrator may have blocked API tokens; use a service account instead.
- **`Failed to resolve cloud_id`** — `_edge/tenant_info` could not be read; pin `cloud_id`.
- **`invalid_client` from a service account** — the client ID or secret is wrong; check with `config validate`.
- **`Multiple Atlassian sites found`** — the credential reaches several sites; pin `cloud_id`.
- **`Invalid Atlassian domain`** — the domain must end with `.atlassian.net`.
- **`Profile 'X' not found`** — no file read defines that profile; check `config list`.
- **Unexpected search fields** — `--fields` wins over `search_default_fields`, which wins over the 17 defaults plus `search_custom_fields`.

---

## Development

Development commands, architecture and compatibility contracts are in [AGENTS.md](AGENTS.md). Report problems at <https://github.com/junyeong-ai/atlassian-cli/issues>.

---

<div align="center">

**🌐 [한국어](README.md)** | **English**

**Version 0.14.0** • Rust 2024 Edition

</div>
