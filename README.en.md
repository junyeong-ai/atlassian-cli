# Atlassian CLI

[![CI](https://github.com/junyeong-ai/atlassian-cli/workflows/CI/badge.svg)](https://github.com/junyeong-ai/atlassian-cli/actions/workflows/ci.yml)
[![Security](https://github.com/junyeong-ai/atlassian-cli/workflows/Security/badge.svg)](https://github.com/junyeong-ai/atlassian-cli/actions/workflows/security.yml)
[![Rust](https://img.shields.io/badge/rust-1.97.1%2B%20(2024%20edition)-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Version](https://img.shields.io/badge/version-0.10.0-blue?style=flat-square)](https://github.com/junyeong-ai/atlassian-cli/releases)
[![DeepWiki](https://img.shields.io/badge/DeepWiki-junyeong--ai%2Fatlassian--cli-blue.svg?logo=data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACwAAAAyCAYAAAAnWDnqAAAAAXNSR0IArs4c6QAAA05JREFUaEPtmUtyEzEQhtWTQyQLHNak2AB7ZnyXZMEjXMGeK/AIi+QuHrMnbChYY7MIh8g01fJoopFb0uhhEqqcbWTp06/uv1saEDv4O3n3dV60RfP947Mm9/SQc0ICFQgzfc4CYZoTPAswgSJCCUJUnAAoRHOAUOcATwbmVLWdGoH//PB8mnKqScAhsD0kYP3j/Yt5LPQe2KvcXmGvRHcDnpxfL2zOYJ1mFwrryWTz0advv1Ut4CJgf5uhDuDj5eUcAUoahrdY/56ebRWeraTjMt/00Sh3UDtjgHtQNHwcRGOC98BJEAEymycmYcWwOprTgcB6VZ5JK5TAJ+fXGLBm3FDAmn6oPPjR4rKCAoJCal2eAiQp2x0vxTPB3ALO2CRkwmDy5WohzBDwSEFKRwPbknEggCPB/imwrycgxX2NzoMCHhPkDwqYMr9tRcP5qNrMZHkVnOjRMWwLCcr8ohBVb1OMjxLwGCvjTikrsBOiA6fNyCrm8V1rP93iVPpwaE+gO0SsWmPiXB+jikdf6SizrT5qKasx5j8ABbHpFTx+vFXp9EnYQmLx02h1QTTrl6eDqxLnGjporxl3NL3agEvXdT0WmEost648sQOYAeJS9Q7bfUVoMGnjo4AZdUMQku50McDcMWcBPvr0SzbTAFDfvJqwLzgxwATnCgnp4wDl6Aa+Ax283gghmj+vj7feE2KBBRMW3FzOpLOADl0Isb5587h/U4gGvkt5v60Z1VLG8BhYjbzRwyQZemwAd6cCR5/XFWLYZRIMpX39AR0tjaGGiGzLVyhse5C9RKC6ai42ppWPKiBagOvaYk8lO7DajerabOZP46Lby5wKjw1HCRx7p9sVMOWGzb/vA1hwiWc6jm3MvQDTogQkiqIhJV0nBQBTU+3okKCFDy9WwferkHjtxib7t3xIUQtHxnIwtx4mpg26/HfwVNVDb4oI9RHmx5WGelRVlrtiw43zboCLaxv46AZeB3IlTkwouebTr1y2NjSpHz68WNFjHvupy3q8TFn3Hos2IAk4Ju5dCo8B3wP7VPr/FGaKiG+T+v+TQqIrOqMTL1VdWV1DdmcbO8KXBz6esmYWYKPwDL5b5FA1a0hwapHiom0r/cKaoqr+27/XcrS5UwSMbQAAAABJRU5ErkJggg==)](https://deepwiki.com/junyeong-ai/atlassian-cli)

> **🌐 [한국어](README.md)** | **English**

---

> **⚡ Fast and Powerful Atlassian Cloud Command-Line Tool**
>
> - 🚀 **Single binary** (no runtime required)
> - 🎯 **60-70% response optimization** (field filtering)
> - 📄 **Full pagination** (fetch all results with `--all`)
> - 📝 **Markdown conversion** (`--format markdown` for HTML→Markdown)
> - 🔧 **Layered config** (CLI → ENV → `--config` → Project → Global)

---

## ⚡ Quick Start (1 minute)

```bash
# 1. Install
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | bash

# 2. Initialize config
atlassian-cli config init --global

# 3. Edit config (pick oauth / service_account / scoped_token / basic — see Authentication)
atlassian-cli config edit --global

# 4. (oauth only) Sign in once via the browser
atlassian-cli auth login

# 5. Validate credentials
atlassian-cli config validate

# 6. Start using
atlassian-cli jira search "status = Open" --limit 5
atlassian-cli confluence search "type=page" --limit 10
```

---

## 🎯 Key Features

### Jira Operations
```bash
# Search issues (JQL)
atlassian-cli jira search "project = PROJ AND status = Open" --limit 10
atlassian-cli jira search "assignee = currentUser()" --fields key,summary,status
atlassian-cli jira search "status = Open" --format markdown  # ADF → Markdown
atlassian-cli jira search "project = PROJ" --all             # Fetch all results
atlassian-cli jira search "project = PROJ" --all --stream    # JSONL streaming

# Get/Create/Update issues
atlassian-cli jira get PROJ-123
atlassian-cli jira get PROJ-123 --format markdown  # description as Markdown
atlassian-cli jira create PROJ "Bug fix" Bug --description "Details"
atlassian-cli jira update PROJ-123 '{"summary":"New title"}'

# Comment/Transition
atlassian-cli jira comment add PROJ-123 "Work completed"
atlassian-cli jira comment update PROJ-123 10042 "Edited comment"
atlassian-cli jira transition list PROJ-123
atlassian-cli jira transition apply PROJ-123 31
atlassian-cli jira delete PROJ-123 --yes        # permanent delete (--yes required)

# Links, worklogs, watchers
atlassian-cli jira link add PROJ-1 PROJ-2 --type Blocks
atlassian-cli jira worklog add PROJ-123 "2h 30m" --comment "Investigation"
atlassian-cli jira watcher add PROJ-123

# Agile — boards, sprints, epics
atlassian-cli jira sprint list --project PROJ
atlassian-cli jira sprint move 55 PROJ-1 PROJ-2
atlassian-cli jira epic assign EPIC-1 PROJ-1
```

### Confluence Operations
```bash
# Search pages (CQL)
atlassian-cli confluence search "type=page AND space=TEAM" --limit 10
atlassian-cli confluence search "type=page" --all           # Fetch all results
atlassian-cli confluence search "type=page" --all --stream  # JSONL streaming
atlassian-cli confluence search "type=page" --format markdown  # Markdown conversion (body included by default)

# Get page (Markdown conversion)
atlassian-cli confluence get 123456 --format markdown

# Get/Create/Update pages
atlassian-cli confluence get 123456                          # HTML format (default)
atlassian-cli confluence get 123456 --format markdown        # Markdown conversion
atlassian-cli confluence create TEAM "API Docs" "<p>Content</p>"
atlassian-cli confluence update 123456 "New Title" "<p>New content</p>"

# Children & comments — every reply, footer and inline
atlassian-cli confluence children 123456
atlassian-cli confluence comment list 123456 --format markdown
atlassian-cli confluence comment list 123456 --location inline   # anchored comments only
atlassian-cli confluence comment list 123456 --roots-only        # top level only
atlassian-cli confluence comment get 67890                       # one comment by id
atlassian-cli confluence comment replies 67890                   # one thread

# Comments / labels / properties / spaces / attachments
atlassian-cli confluence comment add 123456 "<p>Looks good</p>" --reply-to 67890
atlassian-cli confluence label add 123456 needs-review
atlassian-cli confluence property set 123456 review '{"status":"done"}'
atlassian-cli confluence space list
atlassian-cli confluence attachment upload 123456 ./diagram.png
```

### Config & Optimization
```bash
# Config management
atlassian-cli config show            # Show config (masked token)
atlassian-cli config path            # Config file path
atlassian-cli config edit            # Edit with default editor

# JSON output
atlassian-cli jira get PROJ-123 | jq -r '.fields.summary'
```

**Important Notes**:
- Field optimization: 17 default fields (excludes `description`, `id`, `renderedFields`)
- Project filter: `projects_filter` auto-injects into JQL
- ADF auto-conversion: Plain text → Atlassian Document Format

---

## 📦 Installation

### Method 1: Prebuilt Binary (Recommended) ⭐

**Automated install**:
```bash
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | bash
```
Installs the latest prebuilt binary, which then deploys the `jira-confluence` Claude Code skill it carries to `~/.claude/skills`.

```bash
# Install a specific release
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | ATLASSIAN_CLI_VERSION=v0.10.0 bash

# After that the binary manages itself — no need to re-run the installer
atlassian-cli self update
atlassian-cli self uninstall --yes
```

**Manual install**:
1. Download binary from [Releases](https://github.com/junyeong-ai/atlassian-cli/releases)
2. Extract: `tar -xzf atlassian-cli-*.tar.gz`
3. Move to PATH: `mv atlassian-cli ~/.local/bin/`

**Supported Platforms**:
- Linux: x86_64, aarch64
- macOS: Intel (x86_64), Apple Silicon (aarch64)
- Windows: x86_64

`install.sh` supports Linux/macOS automation. Windows builds are published as release binaries for manual installation. Every platform publishes a `.tar.gz`, so `self update` works everywhere.

### Managing the installation

```bash
atlassian-cli self status                     # version, paths, skill state, profiles holding tokens (no network)
atlassian-cli self update                     # replace with the latest release
atlassian-cli self update --version 0.9.0     # a specific one; going back takes an explicit version
atlassian-cli self update --verify-attestations   # also check GitHub build provenance (needs the gh CLI)
atlassian-cli self skill install               # redeploy the skill
atlassian-cli self skill remove --yes
atlassian-cli self uninstall --yes [--keep-skill] [--keep-credentials] [--purge-config]
```

- The skill is compiled into the binary, so the two cannot be different versions, and `self status` compares the deployed copy **byte for byte** — a locally edited one reads as `stale`.
- `self update` runs the downloaded binary and checks the version it reports **before** replacing anything, so a binary that will not run on this machine leaves the installation untouched.
- `self uninstall` also clears OAuth tokens from the OS keychain (`--keep-credentials` to keep them), and **refuses** when a keychain exists but will not be read — locked, or `ATLASSIAN_NO_KEYCHAIN` forbidding the look — because leaving tokens behind with no tool that knows where they are is the worst outcome. A keychain that will not answer takes two runs: the first clears `credentials.json` and refuses, naming what it removed, and `--keep-credentials` finishes the second. `auth logout` exits non-zero there for the same reason — every released target has a keychain compiled in, so "could not reach it" is not "there was nothing in it". `--purge-config` removes the config file this tool writes and the directory only if that leaves it empty, so `credentials.json` survives alongside `--keep-credentials`. Project-local `.atlassian.toml` is never touched.

### Method 2: Build from Source

```bash
git clone https://github.com/junyeong-ai/atlassian-cli
cd atlassian-cli
cargo build --release   # toolchain pinned by rust-toolchain.toml (1.97.1)
cp target/release/atlassian-cli ~/.local/bin/
```

**Requirements**: Rust 1.97.1+

### 🤖 Claude Code Skill

`scripts/install.sh` deploys it to `~/.claude/skills/jira-confluence`, where it is available in every project. Manage it afterwards with `self skill install` / `self skill remove`.

---

## 🔑 Authentication

Pick one explicitly (no auto-detection):

| Method | Principal | Request host | Notes |
|---|---|---|---|
| `oauth` ⭐ | the signed-in user | `api.atlassian.com` | 3LO + PKCE; tokens stored in OS keychain, auto-refreshed |
| `service_account` | non-human SA | `api.atlassian.com` | OAuth 2.0 client_credentials; for CI / automation |
| `scoped_token` | API-token owner | `api.atlassian.com` | **API token with scopes only**; permissions limited to the scopes granted to the token |
| `basic` | API-token owner | `{domain}` | **classic (unscoped) token only** |

The two token methods do not accept each other's tokens: the site host silently
ignores a token carrying scopes and answers 401, and the gateway rejects a
classic one. On a 401 the CLI's `hint` field names the other method. Issue
either kind at <https://id.atlassian.com/manage-profile/security/api-tokens>.

### OAuth 2.0 (3LO) — recommended

Sign in once via the browser; tokens persist in the OS keychain (with a 0600
file fallback) and refresh ~5 minutes before expiry.

```toml
# ~/.config/atlassian-cli/config.toml
[default.auth]
method = "oauth"
client_id = "..."          # issued at developer.atlassian.com
client_secret = "..."      # prefer ATLASSIAN_CLIENT_SECRET env var
redirect_port = 8976       # must match the Callback URL on the OAuth app
# scopes defaults to ["read:jira-user", "read:jira-work", "write:jira-work", "offline_access"]
# Add Confluence scopes only after granting them on the OAuth app:
#   scopes = ["read:jira-user", "read:jira-work", "write:jira-work",
#             "read:confluence-content.all", "read:confluence-space.summary",
#             "write:confluence-content", "offline_access"]
# board/sprint/epic (agile) commands need extra Jira Software scopes beyond the defaults:
#   "read:board-scope:jira-software", "read:sprint:jira-software",
#   "write:sprint:jira-software", "read:epic:jira-software"
# cloud_id = "..."          # pin one site when the user has access to many
```

> **Headless / AI agent**: on a desktop OS the keychain may block with a GUI prompt. Set `ATLASSIAN_NO_KEYCHAIN=1` to skip the keychain and use the 0600 file store. Treat it as a per-environment setting (don't toggle it on/off) — while set, `auth logout` clears only the file store. If you previously logged in with the keychain, run `auth logout` once without the flag to clear it.

```bash
atlassian-cli auth login       # browser → Atlassian → tokens persisted
atlassian-cli auth status      # expiry, scopes, storage backend
atlassian-cli auth refresh     # force refresh (debugging)
atlassian-cli auth logout      # clears stored tokens
```

Prereqs at <https://developer.atlassian.com/console/myapps/>:
1. Create an OAuth 2.0 (3LO) app.
2. Add `http://127.0.0.1:8976/callback` as the Callback URL (port must match `redirect_port`).
3. Grant the scopes you list in config — unscoped scopes are rejected at consent.
4. Copy `client_id` / `client_secret` from Settings.

### Service Account / API tokens — environment variables

```bash
# Service account (CI / automation)
export ATLASSIAN_AUTH_METHOD=service_account
export ATLASSIAN_CLIENT_ID="..."
export ATLASSIAN_CLIENT_SECRET="..."
# ATLASSIAN_CLOUD_ID is optional when the credential accesses exactly one site

# Scoped API token (token created with scopes)
export ATLASSIAN_AUTH_METHOD=scoped_token
export ATLASSIAN_DOMAIN="company.atlassian.net"   # resolves cloud_id
export ATLASSIAN_EMAIL="user@example.com"
export ATLASSIAN_API_TOKEN="..."
# ATLASSIAN_CLOUD_ID pins the site directly; the domain is then unnecessary

# Basic (classic, unscoped API token)
export ATLASSIAN_AUTH_METHOD=basic
export ATLASSIAN_DOMAIN="company.atlassian.net"
export ATLASSIAN_EMAIL="user@example.com"
export ATLASSIAN_API_TOKEN="..."
```

Atlassian is retiring classic tokens — every token issued before 2024-12-15
expired between 2026-03-14 and 2026-05-12, and newly issued tokens carry
scopes. Prefer `scoped_token` for anything created from now on.

`scoped_token` resolves `cloud_id` from `https://{domain}/_edge/tenant_info`,
which needs no credentials. That lookup is the only part of this method that
touches the site host, and it runs once per invocation — pin `cloud_id` to skip
it entirely, or when the site host does not answer from your network. The value
also appears in the site URL at <https://admin.atlassian.com>.

Blank env vars are treated as **absent** — `export VAR=""` no longer shadows
the config-file value.

### Config file

**Locations** (highest priority first within each scope):
- Custom path: `--config <file>`
- Project: `./.atlassian.toml` or `./.atlassian/config.toml` (walked upward from cwd)
- Global: `~/.config/atlassian-cli/config.toml`

Generate a starter with `atlassian-cli config init --global`. The template
ships all four auth methods as commented examples.

### Field optimization (optional env)

```bash
export JIRA_SEARCH_DEFAULT_FIELDS="key,summary,status"
export JIRA_SEARCH_CUSTOM_FIELDS="customfield_10015"
export RESPONSE_EXCLUDE_FIELDS="self,avatarUrls,iconUrl"
```

### Config Priority

```
CLI flags > Environment variables > `--config` file > Project config > Global config
```

---

## 🏗️ Core Architecture

Layered config priority, ADF auto-conversion, field optimization (17 default fields), cursor-based pagination.
For detailed architecture, see [CLAUDE.md](CLAUDE.md).

---

## 🔧 Troubleshooting

### Config Not Found

```bash
# Check config
atlassian-cli config path
atlassian-cli config show

# Reinitialize
atlassian-cli config init --global
```

### API Authentication Failed

**Checklist**:
- [ ] Domain format: `company.atlassian.net` (without https://)
- [ ] Email format valid
- [ ] Token correct (watch for copy/paste spaces)
- [ ] Token shape matches the method — a token with scopes needs `scoped_token`,
      a classic one needs `basic`. On a 401 the error's `hint` names the method
      to switch to.

### `Failed to resolve cloud_id from ...`

`scoped_token` reads the cloud ID from `https://{domain}/_edge/tenant_info`.
When that host is unreachable from your network, pin the value instead via
`ATLASSIAN_CLOUD_ID` or `[auth].cloud_id`; the domain is then unnecessary.

### Field Filtering Not Working

**Priority check**:
1. CLI `--fields` (highest priority)
2. `JIRA_SEARCH_DEFAULT_FIELDS` environment variable
3. Default 17 fields + `JIRA_SEARCH_CUSTOM_FIELDS`

```bash
# Test
JIRA_SEARCH_DEFAULT_FIELDS="key,summary" atlassian-cli jira search "project = PROJ"
```

### Project Filter Auto-Injection

With `projects_filter` config, JQL auto-injected:
```
Input: status = Open
Executed: project IN (PROJ1,PROJ2) AND (status = Open)
```

---

## 📚 Command Reference

### Jira Commands

| Command | Description | Example |
|---------|-------------|---------|
| `get <KEY>` | Get issue | `jira get PROJ-123` |
| `get <KEY> --format markdown` | Get issue (Markdown) | `jira get PROJ-123 --format markdown` |
| `search <JQL>` | JQL search | `jira search "status = Open" --limit 10` |
| `search <JQL> --all` | Fetch all results | `jira search "project = PROJ" --all` |
| `search <JQL> --all --stream` | JSONL streaming | `jira search "project = PROJ" --all --stream` |
| `search <JQL> --format markdown` | JQL search (Markdown) | `jira search "status = Open" --format markdown` |
| `create <PROJECT> <SUMMARY> <TYPE>` | Create issue | `jira create PROJ "Title" Bug` |
| `update <KEY> <JSON>` | Update issue | `jira update PROJ-123 '{"summary":"New"}'` |
| `delete <KEY> --yes [--delete-subtasks]` | Delete issue (irreversible) | `jira delete PROJ-123 --yes` |
| `comment add <KEY> <TEXT>` | Add comment | `jira comment add PROJ-123 "Done"` |
| `comment update <KEY> <COMMENT_ID> <TEXT>` | Update comment | `jira comment update PROJ-123 10042 "Done"` |
| `comment list <KEY>` | List comments | `jira comment list PROJ-123` |
| `comment delete <KEY> <COMMENT_ID>` | Delete comment | `jira comment delete PROJ-123 10042` |
| `transition list <KEY>` | List transitions | `jira transition list PROJ-123` |
| `transition apply <KEY> <ID>` | Transition issue | `jira transition apply PROJ-123 31` |
| `link add/remove/list`, `link types` | Issue links | `jira link add PROJ-1 PROJ-2 --type Blocks` |
| `worklog add/list/update/remove` | Time tracking | `jira worklog add PROJ-123 "2h 30m"` |
| `watcher add/remove/list <KEY>` | Watchers | `jira watcher add PROJ-123` |
| `list types/priorities/statuses/labels` | Global metadata | `jira list types` |
| `board list --project <KEY>` | Agile boards | `jira board list --project PROJ` |
| `sprint list/move/backlog` | Sprints / backlog | `jira sprint move 55 PROJ-1 PROJ-2` |
| `epic assign/unassign <EPIC> <KEY...>` | Epic membership | `jira epic assign EPIC-1 PROJ-1` |

### Confluence Commands

| Command | Description | Example |
|---------|-------------|---------|
| `search <CQL>` | CQL search | `confluence search "type=page" --limit 10` |
| `search <CQL> --format markdown` | CQL search (Markdown) | `confluence search "type=page" --format markdown` |
| `get <ID>` | Get page | `confluence get 123456` |
| `get <ID> --format markdown` | Get page (Markdown) | `confluence get 123456 --format markdown` |
| `create <SPACE> <TITLE> <CONTENT> [--parent <ID>]` | Create page (nest under a parent with `--parent`) | `confluence create TEAM "Title" "<p>HTML</p>" --parent 12345` |
| `update <ID> <TITLE> <CONTENT>` | Update page | `confluence update 123456 "Title" "<p>HTML</p>"` |
| `delete <ID> --yes` | Delete page (to trash) | `confluence delete 123456 --yes` |
| `children <ID>` | List children | `confluence children 123456` |
| `comment list <ID> [--location footer\|inline] [--roots-only]` | Every comment on a page — both families, replies included; each entry carries `location`, `depth`, `parentCommentId` | `confluence comment list 123456` |
| `comment get <COMMENT_ID> [--location ...]` | One comment by id (defaults to `footer`) | `confluence comment get 67890` |
| `comment replies <COMMENT_ID> [--location ...]` | The whole thread under one comment | `confluence comment replies 67890` |
| `comment add/update/delete ...` | Footer comment writes | `confluence comment add 123456 "<p>Hi</p>" --reply-to 67890` |
| `label list/add/remove <ID> [LABEL]` | Page labels | `confluence label add 123456 needs-review` |
| `property list/set/delete <ID> [KEY] [JSON]` | Content properties (value is strict JSON) | `confluence property set 123456 review '{"status":"done"}'` |
| `space list`, `space get <KEY>` | Spaces | `confluence space get TEAM` |
| `attachment list/upload <ID> [FILE] [--content-type <MIME>]` | Attachments (Content-Type auto-mapped from extension; `--content-type` overrides) | `confluence attachment upload 123456 ./a.png` |

### Config Commands

| Command | Description | Example |
|---------|-------------|---------|
| `init [--global]` | Initialize config | `config init --global` |
| `show` | Show config | `config show` |
| `edit [--global]` | Edit with editor | `config edit` |
| `path [--global]` | File path | `config path` |
| `list` | List locations | `config list` |
| `validate` | Validate auth and Cloud access; individual APIs still require scopes/permissions | `config validate` |

### Common Options

| Option | Description | Applies To |
|--------|-------------|------------|
| `--domain` | Override domain | All commands |
| `--email` | Override email | All commands |
| `--token` | Override token | All commands |
| `--profile <NAME>` | Select config profile | Global |
| `--config <PATH>` | Override config path | Global |
| `--pretty` | Pretty-print JSON | Global |
| `-v` / `-vv` / `-vvv` | stderr logging level | Global |
| `--limit <N>` | Limit results | search |
| `--all` | All results (pagination) | jira search, confluence search |
| `--stream` | JSONL streaming | jira search, confluence search (requires --all) |
| `--expand` | Additional expand fields (ancestors, etc.; body.storage included by default) | confluence search |
| `--format` | Output format (html, markdown) | jira get/search, confluence search/get/comments |
| `--fields` | Specify fields | jira search, jira get |

### Errors & exit codes

Failures print a single-line JSON object to **stderr** (stdout carries results
only, so `| jq` pipelines never see partial output):

```json
{"error":{"message":"Failed to get issue (404 Not Found): ...","operation":"get issue","status":404}}
```

API failures include `status` and `operation`; a `hint` field appears when a
known remediation exists (e.g. an API token sent to the host that refuses its
shape). Rate
limits (429) are retried automatically up to 3 times, honoring `Retry-After`.

| Exit code | Meaning |
|---|---|
| `0` | success |
| `1` | generic failure |
| `2` | CLI usage error |
| `3` | authentication / permission (401, 403) |
| `4` | not found (404) |
| `5` | rate limited (429, retries exhausted) |
| `6` | server error (5xx) |

### Shell completion

```bash
atlassian-cli completions zsh > "${fpath[1]}/_atlassian-cli"   # zsh
atlassian-cli completions bash > /etc/bash_completion.d/atlassian-cli
```

Supported: `bash`, `zsh`, `fish`, `elvish`, `powershell`.

---

## 🚀 Developer Guide

**Architecture, debugging, contribution guide**: See [CLAUDE.md](CLAUDE.md)

---

## 💬 Support

- **GitHub Issues**: [Report issues](https://github.com/junyeong-ai/atlassian-cli/issues)
- **Developer Docs**: [CLAUDE.md](CLAUDE.md)

---

<div align="center">

**🌐 [한국어](README.md)** | **English**

**Version 0.10.0** • Rust 2024 Edition

Made with ❤️ for productivity

</div>
