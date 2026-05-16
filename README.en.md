# Atlassian CLI

[![CI](https://github.com/junyeong-ai/atlassian-cli/workflows/CI/badge.svg)](https://github.com/junyeong-ai/atlassian-cli/actions/workflows/ci.yml)
[![Security](https://github.com/junyeong-ai/atlassian-cli/workflows/Security/badge.svg)](https://github.com/junyeong-ai/atlassian-cli/actions/workflows/security.yml)
[![Rust](https://img.shields.io/badge/rust-1.95.0%2B%20(2024%20edition)-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Version](https://img.shields.io/badge/version-0.4.0-blue?style=flat-square)](https://github.com/junyeong-ai/atlassian-cli/releases)
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

# 3. Edit config (pick oauth / service_account / basic — see Authentication)
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
atlassian-cli jira transitions PROJ-123
atlassian-cli jira transition PROJ-123 31
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

# Children/Comments
atlassian-cli confluence children 123456
atlassian-cli confluence comments 123456 --format markdown
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
Installs the latest prebuilt binary and can install the `jira-confluence` Claude Code skill at user level (`~/.claude/skills`). When run through `curl | bash`, the installer fetches the skill directly from GitHub.

```bash
# Install a specific release
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | ATLASSIAN_CLI_VERSION=v0.4.0 bash

# Uninstall (non-interactive defaults keep skill/config)
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/uninstall.sh | bash

# Remove skill and global config too
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/uninstall.sh | bash -s -- --yes
```

**Manual install**:
1. Download binary from [Releases](https://github.com/junyeong-ai/atlassian-cli/releases)
2. Extract: `tar -xzf atlassian-cli-*.tar.gz`
3. Move to PATH: `mv atlassian-cli ~/.local/bin/`

**Supported Platforms**:
- Linux: x86_64, aarch64
- macOS: Intel (x86_64), Apple Silicon (aarch64)
- Windows: x86_64

`install.sh` supports Linux/macOS automation. Windows builds are published as release binaries for manual installation.

### Method 2: Build from Source

```bash
git clone https://github.com/junyeong-ai/atlassian-cli
cd atlassian-cli
cargo +1.95.0 build --release
cp target/release/atlassian-cli ~/.local/bin/
```

**Requirements**: Rust 1.95.0+

### 🤖 Claude Code Skill (Optional)

When running `scripts/install.sh`, you can choose to install the Claude Code skill:

- **User-level** (recommended): Available in all projects via `~/.claude/skills/jira-confluence`
- **Skip**: Manual installation later

The installer uses the local skill definition when run inside a checkout. With `curl | bash`, it fetches the skill from GitHub.

---

## 🔑 Authentication

Pick one explicitly (no auto-detection):

| Method | Principal | Notes |
|---|---|---|
| `oauth` ⭐ | the signed-in user | 3LO + PKCE; tokens stored in OS keychain, auto-refreshed |
| `service_account` | non-human SA | OAuth 2.0 client_credentials; for CI / automation |
| `basic` | API-token owner | personal token from <https://id.atlassian.com/manage-profile/security/api-tokens> |

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
# cloud_id = "..."          # pin one site when the user has access to many
```

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

### Service Account / Basic — environment variables

```bash
# Service account (CI / automation)
export ATLASSIAN_AUTH_METHOD=service_account
export ATLASSIAN_CLIENT_ID="..."
export ATLASSIAN_CLIENT_SECRET="..."
# ATLASSIAN_CLOUD_ID is optional when the credential accesses exactly one site

# Basic (personal API token)
export ATLASSIAN_AUTH_METHOD=basic
export ATLASSIAN_DOMAIN="company.atlassian.net"
export ATLASSIAN_EMAIL="user@example.com"
export ATLASSIAN_API_TOKEN="..."
```

Blank env vars are treated as **absent** — `export VAR=""` no longer shadows
the config-file value.

### Config file

**Locations** (highest priority first within each scope):
- Custom path: `--config <file>`
- Project: `./.atlassian.toml` or `./.atlassian/config.toml` (walked upward from cwd)
- Global: `~/.config/atlassian-cli/config.toml`

Generate a starter with `atlassian-cli config init --global`. The template
ships all three auth methods as commented examples.

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
| `comment add <KEY> <TEXT>` | Add comment | `jira comment add PROJ-123 "Done"` |
| `comment update <KEY> <COMMENT_ID> <TEXT>` | Update comment | `jira comment update PROJ-123 10042 "Done"` |
| `comments <KEY>` | List comments | `jira comments PROJ-123` |
| `transitions <KEY>` | List transitions | `jira transitions PROJ-123` |
| `transition <KEY> <ID>` | Transition issue | `jira transition PROJ-123 31` |

### Confluence Commands

| Command | Description | Example |
|---------|-------------|---------|
| `search <CQL>` | CQL search | `confluence search "type=page" --limit 10` |
| `search <CQL> --format markdown` | CQL search (Markdown) | `confluence search "type=page" --format markdown` |
| `get <ID>` | Get page | `confluence get 123456` |
| `get <ID> --format markdown` | Get page (Markdown) | `confluence get 123456 --format markdown` |
| `create <SPACE> <TITLE> <CONTENT>` | Create page | `confluence create TEAM "Title" "<p>HTML</p>"` |
| `update <ID> <TITLE> <CONTENT>` | Update page | `confluence update 123456 "Title" "<p>HTML</p>"` |
| `children <ID>` | List children | `confluence children 123456` |
| `comments <ID>` | Get comments | `confluence comments 123456` |
| `comments <ID> --format markdown` | Get comments (Markdown) | `confluence comments 123456 --format markdown` |

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

**Version 0.4.0** • Rust 2024 Edition

Made with ❤️ for productivity

</div>
