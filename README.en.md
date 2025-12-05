# Atlassian CLI

[![CI](https://github.com/junyeong-ai/atlassian-cli/workflows/CI/badge.svg)](https://github.com/junyeong-ai/atlassian-cli/actions)
[![Lint](https://github.com/junyeong-ai/atlassian-cli/workflows/Lint/badge.svg)](https://github.com/junyeong-ai/atlassian-cli/actions)
[![Rust](https://img.shields.io/badge/rust-1.91.1%2B%20(2024%20edition)-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Version](https://img.shields.io/badge/version-0.1.0-blue?style=flat-square)](https://github.com/junyeong-ai/atlassian-cli/releases)

> **🌐 [한국어](README.md)** | **English**

---

> **⚡ Fast and Powerful Atlassian Cloud Command-Line Tool**
>
> - 🚀 **Single binary** (no runtime required)
> - 🎯 **60-70% response optimization** (field filtering)
> - 📄 **Full pagination** (fetch all results with `--all`)
> - 📝 **Markdown conversion** (`--format markdown` for HTML→Markdown)
> - 🔧 **4-tier config** (CLI → ENV → Project → Global)

---

## ⚡ Quick Start (1 minute)

```bash
# 1. Install
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | bash

# 2. Initialize config
atlassian-cli config init --global

# 3. Edit config (enter domain, email, token)
atlassian-cli config edit --global

# 4. Start using! 🎉
atlassian-cli jira search "status = Open" --limit 5
atlassian-cli confluence search "type=page" --limit 10
```

**Tip**: [Generate API Token](https://id.atlassian.com/manage-profile/security/api-tokens) required

---

## 🎯 Key Features

### Jira Operations
```bash
# Search issues (JQL)
atlassian-cli jira search "project = PROJ AND status = Open" --limit 10
atlassian-cli jira search "assignee = currentUser()" --fields key,summary,status
atlassian-cli jira search "status = Open" --format markdown  # ADF → Markdown

# Get/Create/Update issues
atlassian-cli jira get PROJ-123
atlassian-cli jira get PROJ-123 --format markdown  # description as Markdown
atlassian-cli jira create PROJ "Bug fix" Bug --description "Details"
atlassian-cli jira update PROJ-123 '{"summary":"New title"}'

# Comment/Transition
atlassian-cli jira comment add PROJ-123 "Work completed"
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

**Manual install**:
1. Download binary from [Releases](https://github.com/junyeong-ai/atlassian-cli/releases)
2. Extract: `tar -xzf atlassian-cli-*.tar.gz`
3. Move to PATH: `mv atlassian-cli ~/.local/bin/`

**Supported Platforms**:
- Linux: x86_64, aarch64
- macOS: Intel (x86_64), Apple Silicon (aarch64)
- Windows: x86_64

### Method 2: Build from Source

```bash
git clone https://github.com/junyeong-ai/atlassian-cli
cd atlassian-cli
cargo build --release
cp target/release/atlassian-cli ~/.local/bin/
```

**Requirements**: Rust 1.91.1+

### 🤖 Claude Code Skill (Optional)

When running `./scripts/install.sh`, you can choose to install the Claude Code skill:

- **User-level** (recommended): Available in all projects
- **Project-level**: Auto-distributed to team via git
- **Skip**: Manual installation later

---

## 🔑 Generate API Token

1. Visit [Atlassian API Tokens](https://id.atlassian.com/manage-profile/security/api-tokens)
2. Click "Create API token"
3. Enter label (e.g., "atlassian-cli")
4. Copy token and add to config

**Security**: Treat token like password. Regenerate immediately if exposed.

---

## ⚙️ Configuration

### Environment Variables

```bash
export ATLASSIAN_DOMAIN="company.atlassian.net"
export ATLASSIAN_EMAIL="user@example.com"
export ATLASSIAN_API_TOKEN="your-token"

# Field optimization
export JIRA_SEARCH_DEFAULT_FIELDS="key,summary,status"
export JIRA_SEARCH_CUSTOM_FIELDS="customfield_10015"
```

### Config File

**Location**:
- macOS/Linux: `~/.config/atlassian-cli/config.toml`
- Windows: `%APPDATA%\atlassian-cli\config.toml`
- Project: `./.atlassian.toml`

**Default config** (generated by `atlassian-cli config init`):
```toml
[default]
domain = "company.atlassian.net"
email = "user@example.com"
token = "your-api-token"

[default.jira]
projects_filter = ["PROJ1", "PROJ2"]

[default.confluence]
spaces_filter = ["TEAM", "DOCS"]

[default.performance]
request_timeout_ms = 30000
rate_limit_delay_ms = 200
```

### Config Priority

```
CLI flags > Environment variables > Project config > Global config
```

---

## 🏗️ Core Architecture

4-tier config priority, ADF auto-conversion, field optimization (17 default fields), cursor-based pagination.
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
| `search <JQL> --format markdown` | JQL search (Markdown) | `jira search "status = Open" --format markdown` |
| `create <PROJECT> <SUMMARY> <TYPE>` | Create issue | `jira create PROJ "Title" Bug` |
| `update <KEY> <JSON>` | Update issue | `jira update PROJ-123 '{"summary":"New"}'` |
| `comment add <KEY> <TEXT>` | Add comment | `jira comment add PROJ-123 "Done"` |
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
| `validate` | Validate API connection | `config validate` |

### Common Options

| Option | Description | Applies To |
|--------|-------------|------------|
| `--domain` | Override domain | All commands |
| `--email` | Override email | All commands |
| `--token` | Override token | All commands |
| `--limit <N>` | Limit results | search |
| `--all` | All results (pagination) | confluence search |
| `--stream` | JSONL streaming | confluence search (requires --all) |
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

**Version 0.1.0** • Rust 2024 Edition

Made with ❤️ for productivity

</div>
