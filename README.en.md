# Atlassian CLI

[![CI](https://github.com/junyeong-ai/atlassian-cli/workflows/CI/badge.svg)](https://github.com/junyeong-ai/atlassian-cli/actions)
[![Rust](https://img.shields.io/badge/rust-1.91.1%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)

English | **[한국어](README.md)**

Access Jira and Confluence from your terminal.

## Features

- **Single binary** — No runtime dependencies
- **60-70% response optimization** — Fetch only needed fields
- **Full pagination** — Get all results with `--all`
- **4-tier config** — CLI > ENV > Project > Global

## Installation

```bash
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | bash
```

Or download from [Releases](https://github.com/junyeong-ai/atlassian-cli/releases)

## Getting Started

```bash
# 1. Initialize config
atlassian-cli config init --global

# 2. Edit config (~/.config/atlassian-cli/config.toml)
atlassian-cli config edit --global
```

```toml
[default]
domain = "company.atlassian.net"
email = "user@example.com"
token = "your-api-token"  # https://id.atlassian.com/manage-profile/security/api-tokens
```

## Usage

### Jira

```bash
# Search
atlassian-cli jira search "status = Open" --limit 10
atlassian-cli jira search "project = PROJ" --fields key,summary,status

# Get/Create/Update
atlassian-cli jira get PROJ-123
atlassian-cli jira create PROJ "Title" Bug --description "Details"
atlassian-cli jira update PROJ-123 '{"summary":"New title"}'

# Comment/Transition
atlassian-cli jira comment add PROJ-123 "Done"
atlassian-cli jira transition PROJ-123 31
```

### Confluence

```bash
# Search
atlassian-cli confluence search "type=page" --limit 10
atlassian-cli confluence search "space=TEAM" --all          # All results
atlassian-cli confluence search "space=TEAM" --all --stream # JSONL streaming

# Get/Create/Update
atlassian-cli confluence get 123456
atlassian-cli confluence create TEAM "Page Title" "<p>Content</p>"
atlassian-cli confluence update 123456 "New Title" "<p>New content</p>"
```

### Config Management

```bash
atlassian-cli config show   # Current config (token masked)
atlassian-cli config path   # Config file path
atlassian-cli config edit   # Open in editor
```

## Environment Variables

```bash
export ATLASSIAN_DOMAIN="company.atlassian.net"
export ATLASSIAN_EMAIL="user@example.com"
export ATLASSIAN_API_TOKEN="your-token"

# Field optimization
export JIRA_SEARCH_DEFAULT_FIELDS="key,summary,status"
```

## Command Summary

| Jira | Confluence | Config |
|------|------------|--------|
| `get` `search` `create` `update` | `get` `search` `create` `update` | `init` `show` `edit` |
| `comment add/update` | `children` `comments` | `path` `list` |
| `transition` `transitions` | | |

### Key Options

| Option | Description |
|--------|-------------|
| `--limit N` | Limit results |
| `--all` | All results (pagination) |
| `--stream` | JSONL streaming (requires `--all`) |
| `--fields` | Specify fields (Jira) |

## Support

- [Issues](https://github.com/junyeong-ai/atlassian-cli/issues)
- [Developer Guide](CLAUDE.md)
