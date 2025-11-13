# Atlassian CLI

> ⚡ **Fast and Powerful Atlassian Cloud CLI Tool**

[![Rust](https://img.shields.io/badge/rust-1.91.1%2B%20(2024%20edition)-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Tests](https://img.shields.io/badge/tests-120%20passing-brightgreen?style=flat-square)](https://github.com/yourusername/atlassian-cli)
[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)

**[한국어](README.md)** | **[English](README.en.md)** | **[AI Agent Guide](CLAUDE.md)**

---

## ⚡ Quick Start (3 Steps)

```bash
# 1. Install
git clone https://github.com/yourusername/atlassian-cli
cd atlassian-cli
./install.sh

# 2. Configure
atlassian config init --global
# Edit ~/.config/atlassian-cli/config.toml

# 3. Start using!
atlassian jira search "project = TMS AND status = Open" --limit 10
atlassian confluence search "type=page AND space=TEAM"
```

---

## 🎯 Why Atlassian CLI?

### 🚀 Fast and Efficient
- **3.8MB Single Binary**: Rust-based native execution
- **Instant Start**: No separate runtime required
- **Low Resources**: Memory efficient

### 💪 Complete Features
- **14 Operations**: 8 Jira + 6 Confluence commands
- **JQL/CQL Support**: Powerful query languages
- **ADF Auto-Conversion**: Plain text → Atlassian Document Format
- **Field Optimization**: 60-70% response size reduction

### 🔧 Flexible Configuration
- **4-Tier Priority**: CLI flags → Environment → Project config → Global config
- **Multi-Profile**: Manage multiple Atlassian instances
- **Project/Space Filtering**: Access control

### ✅ Production Ready
- **120 Tests**: All passing
- **Type Safe**: Rust's strong type system
- **Zero Warnings**: Strict code quality policy

---

## 📦 Installation

### Method 1: Install Script (Recommended)

```bash
git clone https://github.com/yourusername/atlassian-cli
cd atlassian-cli
cargo build --release
./install.sh
```

Binary will be installed to `~/.local/bin/atlassian`.

### Method 2: Cargo

```bash
cargo install --path .
```

### Method 3: Manual Build

```bash
cargo build --release
# Binary: target/release/atlassian
```

### Uninstall

```bash
./uninstall.sh
```

---

## ⚙️ Configuration

### Quick Setup

```bash
# Initialize global config
atlassian config init --global

# Edit config file
atlassian config edit --global
```

### Config File Locations

- **Global**: `~/.config/atlassian-cli/config.toml`
- **Project**: `./.atlassian.toml`

### Basic Configuration

```toml
[default]
domain = "company.atlassian.net"
email = "user@example.com"
token = "your-api-token"

[default.jira]
projects_filter = ["PROJ1", "PROJ2"]

[default.confluence]
spaces_filter = ["TEAM", "DOCS"]
```

### Generate API Token

1. Visit [Atlassian API Tokens](https://id.atlassian.com/manage-profile/security/api-tokens)
2. Click "Create API token"
3. Copy token and add to config file

### Configuration Priority

```
CLI flags > Environment variables > Project config > Global config
```

**Example**:
```bash
# Use CLI flags instead of config file
atlassian --domain company.atlassian.net --email user@example.com --token TOKEN \
  jira search "status = Open"

# Use environment variables
export ATLASSIAN_DOMAIN="company.atlassian.net"
export ATLASSIAN_EMAIL="user@example.com"
export ATLASSIAN_API_TOKEN="your-token"
```

---

## 💡 Usage Examples

### Jira Operations

```bash
# Get issue
atlassian jira get PROJ-123

# JQL search
atlassian jira search "project = PROJ AND status = Open" --limit 10
atlassian jira search "assignee = currentUser() AND status != Done"

# Create issue
atlassian jira create PROJ "Bug fix" Bug --description "Details here"

# Update issue
atlassian jira update PROJ-123 '{"summary":"New title"}'

# Add comment
atlassian jira comment add PROJ-123 "Work completed"

# Transition issue
atlassian jira transitions PROJ-123
atlassian jira transition PROJ-123 31
```

### Confluence Operations

```bash
# Search pages
atlassian confluence search 'type=page AND space="TEAM"' --limit 10

# Get page
atlassian confluence get 123456

# Create page
atlassian confluence create TEAM "API Documentation" "<p>Content here</p>"

# Update page
atlassian confluence update 123456 "API Documentation v2" "<p>New content</p>"

# List children
atlassian confluence children 123456

# Get comments
atlassian confluence comments 123456
```

### Configuration Management

```bash
# Show current config (token masked)
atlassian config show

# Get config file path
atlassian config path --global
atlassian config path

# Edit config file
atlassian config edit --global

# List all config locations
atlassian config list
```

### Advanced Features

#### Field Optimization (60-70% size reduction)

```bash
# Search with default 17 fields (excludes description)
atlassian jira search "project = PROJ"

# Specify custom fields
atlassian jira search "project = PROJ" --fields key,summary,status,assignee

# Override defaults with environment variable
export JIRA_SEARCH_DEFAULT_FIELDS="key,summary,status"
atlassian jira search "project = PROJ"

# Add custom fields to defaults
export JIRA_SEARCH_CUSTOM_FIELDS="customfield_10015,customfield_10016"
```

**Default 17 fields**:
```
key, summary, status, priority, issuetype,
assignee, reporter, creator, created, updated,
duedate, resolutiondate, project, labels,
components, parent, subtasks
```

#### Multi-Profile

```toml
[default]
domain = "company.atlassian.net"
email = "user@company.com"

[work]
domain = "work.atlassian.net"
email = "user@work.com"
```

```bash
atlassian --profile work jira search "project = WORK"
```

#### JSON Output

```bash
# Pretty JSON output
atlassian jira get PROJ-123 --pretty

# Use with jq
atlassian jira search "assignee = currentUser()" | jq -r '.items[].key'
```

---

## 🏗️ Architecture

### Project Structure

```
src/
├── main.rs          # CLI entry point (clap)
├── config.rs        # 4-tier priority config
├── http.rs          # HTTP client
├── jira/
│   ├── api.rs       # 8 Jira operations
│   ├── adf.rs       # ADF auto-conversion
│   └── fields.rs    # Field optimization
└── confluence/
    ├── api.rs       # 6 Confluence operations
    └── fields.rs    # Field optimization
```

### Core Technologies

- **Language**: Rust 2024 Edition (MSRV 1.91.1)
- **CLI**: clap 4.5 (derive API)
- **Async**: Tokio 1.48
- **HTTP**: Reqwest 0.12 (rustls-tls)
- **JSON**: serde_json 1.0

### API Versions

- **Jira**: REST API v3
- **Confluence**: REST API v2 (search uses v1)

---

## 🔧 Troubleshooting

### Config Not Found

**Check**:
- Config file path: `atlassian config path`
- Config content: `atlassian config show`

**Solution**:
```bash
# Initialize global config
atlassian config init --global --domain company.atlassian.net \
  --email user@example.com --token YOUR_TOKEN
```

### API Authentication Failed

**Check**:
- Domain format: `company.atlassian.net` (without https://)
- Email format: Valid email address
- Token: Generated from [API Tokens page](https://id.atlassian.com/manage-profile/security/api-tokens)

### Field Filtering Not Working

**Check priority**:
1. CLI `--fields` parameter
2. `JIRA_SEARCH_DEFAULT_FIELDS` environment variable
3. Default 17 fields + `JIRA_SEARCH_CUSTOM_FIELDS`

```bash
# Test
JIRA_SEARCH_DEFAULT_FIELDS="key,summary" atlassian jira search "project = PROJ"
```

### Project Access Restriction

```toml
[default.jira]
projects_filter = ["PROJ1", "PROJ2"]
```

Auto-injected if no project in JQL:
```
Input: status = Open
Executed: project IN (PROJ1,PROJ2) AND (status = Open)
```

---

## 📚 Resources

### Atlassian API
- [Jira REST API v3](https://developer.atlassian.com/cloud/jira/platform/rest/v3/)
- [Confluence REST API v2](https://developer.atlassian.com/cloud/confluence/rest/v2/)
- [Atlassian Document Format (ADF)](https://developer.atlassian.com/cloud/jira/platform/apis/document/structure/)

### Development Documentation
- [CLAUDE.md](CLAUDE.md) - AI Agent Developer Guide
- [Rust Official Documentation](https://www.rust-lang.org)

---

## 🚀 Development

### Build

```bash
cargo build              # Development build
cargo build --release    # Release build (optimized)
cargo test               # Run tests (120)
cargo clippy             # Lint
cargo fmt                # Format
```

### Release Profile

```toml
[profile.release]
opt-level = 3       # Maximum optimization
lto = true          # Link-time optimization
codegen-units = 1   # Single codegen unit
strip = true        # Strip debug symbols
```

**Result**: 3.8MB optimized binary

---

## 📝 License

MIT License

---

## 🤝 Contributing

Issues and Pull Requests are welcome!

1. Fork
2. Create feature branch (`git checkout -b feature/amazing`)
3. Commit changes (`git commit -m 'Add amazing feature'`)
4. Push branch (`git push origin feature/amazing`)
5. Create Pull Request

---

<div align="center">

**Fast and Powerful Atlassian CLI Tool Built with Rust** 🦀

Version 0.1.0 • Made with ❤️ for productivity

</div>
