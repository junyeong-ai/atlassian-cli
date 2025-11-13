# Atlassian CLI

High-performance command-line interface for Atlassian Jira and Confluence.

## Features

- **Fast**: Rust-powered, 3.8MB binary, optimized for performance
- **Complete**: 14 operations (8 Jira + 6 Confluence)
- **Flexible**: Config files, environment variables, CLI flags with priority
- **Production-ready**: 122 tests passing, field filtering, response optimization

## Installation

### Global Install (Recommended)

```bash
# Clone the repository
git clone https://github.com/yourusername/atlassian-cli.git
cd atlassian-cli

# Run install script
./install.sh
```

This installs the `atlassian` binary to `~/.local/bin` and makes it available globally.

### Alternative: Cargo Install

```bash
cargo install --path .
```

### Uninstall

```bash
./uninstall.sh
```

This removes the binary and optionally removes global configuration files.

## Quick Start

### Configuration

```bash
# Initialize global config
atlassian config init --global

# Edit ~/.config/atlassian-cli/config.toml
# Or use environment variables
export ATLASSIAN_DOMAIN="company.atlassian.net"
export ATLASSIAN_EMAIL="user@example.com"
export ATLASSIAN_API_TOKEN="your-token"
```

### Jira Commands

```bash
# Get issue
atlassian jira get PROJ-123

# Search with JQL
atlassian jira search "project = PROJ AND status = Open" --limit 10

# Create issue
atlassian jira create PROJ "Bug title" Bug --description "Details"

# Update issue
atlassian jira update PROJ-123 '{"summary":"New title"}'

# Add comment
atlassian jira comment add PROJ-123 "Great work!"

# Transition issue
atlassian jira transitions PROJ-123
atlassian jira transition PROJ-123 31
```

### Confluence Commands

```bash
# Search
atlassian confluence search 'type=page AND space="TEAM"' --limit 5

# Get page
atlassian confluence get 123456

# Create page
atlassian confluence create SPACE "Page Title" "<p>Content</p>"

# Update page
atlassian confluence update 123456 "New Title" "<p>New content</p>"

# Get children/comments
atlassian confluence children 123456
atlassian confluence comments 123456
```

### Configuration Management

```bash
# Show current config
atlassian config show

# List all config locations
atlassian config list

# Initialize project config
atlassian config init

# Get config file path
atlassian config path --global  # Global config path
atlassian config path            # Project config path

# Edit config file with default editor
atlassian config edit --global   # Edit global config
atlassian config edit            # Edit project config
```

## Configuration Priority

1. **CLI flags** (highest): `--domain`, `--email`, `--token`
2. **Environment variables**: `ATLASSIAN_*`
3. **Project config**: `./.atlassian.toml`
4. **Global config**: `~/.config/atlassian-cli/config.toml`

## Configuration File Example

```toml
[default]
domain = "company.atlassian.net"
email = "user@example.com"

[default.jira]
projects_filter = ["PROJ1", "PROJ2"]
search_default_fields = ["key", "summary", "status"]

[default.confluence]
spaces_filter = ["TEAM"]

# Multi-tenant support
[work]
domain = "work.atlassian.net"
email = "me@work.com"
```

Use profiles:

```bash
atlassian --profile work jira get WORK-123
```

## Output

All commands output JSON to stdout. Errors go to stderr.

```bash
# Compact JSON (default)
atlassian jira get PROJ-123

# Pretty-printed JSON
atlassian jira get PROJ-123 --pretty

# Pipe to jq
atlassian jira search "assignee = currentUser()" | jq -r '.items[].key'
```

## Advanced Features

### Field Filtering

Optimize token usage by specifying fields:

```bash
# Custom fields
atlassian jira search "project = PROJ" --fields key,summary,status,assignee

# Environment variable
export JIRA_SEARCH_DEFAULT_FIELDS="key,summary,status"
```

### Response Optimization

Automatically removes unnecessary fields (avatarUrls, iconUrl, etc.) for 20-30% token reduction.

```toml
[default.optimization]
response_exclude_fields = ["customField1", "customField2"]
```

### Project/Space Filtering

Restrict access to specific projects or spaces:

```toml
[default.jira]
projects_filter = ["PROJ1", "PROJ2"]

[default.confluence]
spaces_filter = ["SPACE1", "SPACE2"]
```

## Development

```bash
# Build
cargo build --release

# Test
cargo test

# Format
cargo fmt

# Lint
cargo clippy
```

## Architecture

```
src/
├── main.rs          # CLI entry (Clap)
├── lib.rs           # Library exports
├── config.rs        # Config loading with priority
├── http.rs          # HTTP client
├── filter.rs        # Response filtering
├── test_utils.rs    # Test helpers
├── jira/            # 8 Jira operations
│   ├── api.rs
│   ├── adf.rs       # ADF support
│   └── fields.rs    # Field optimization
└── confluence/      # 6 Confluence operations
    ├── api.rs
    └── fields.rs    # Field optimization
```

## Dependencies

- tokio 1.48 - Async runtime
- reqwest 0.12.24 - HTTP client (rustls-tls)
- clap 4.5.51 - CLI parsing
- serde/serde_json 1.0 - JSON serialization
- toml 0.9.8 - Config files
- dirs 6.0.0 - Config paths

All dependencies are latest stable versions as of 2025-11-13.

## License

MIT
