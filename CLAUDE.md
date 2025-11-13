# Atlassian CLI - AI Agent Developer Guide

Quick reference for AI agents maintaining and extending this Rust CLI tool.

## Quick Reference

**What**: Rust CLI for Atlassian with field optimization and ADF support
**Stack**: Rust 2024 (1.91.1+), clap, Tokio, reqwest
**Tests**: `cargo test` (120 tests)
**Binary**: 3.8MB optimized
**Commands**: config (5), jira (8), confluence (6)

---

## Project Structure

```
src/
├── main.rs              # CLI entry: clap Command routing
├── config.rs            # 4-tier priority: CLI > ENV > Project > Global
├── http.rs              # HTTP client factory (rustls, Basic Auth)
├── filter.rs            # Response filter (NOT USED - dead code)
├── test_utils.rs        # Shared test helpers
├── jira/
│   ├── api.rs           # 8 operations (search, get, create, update, comment, transition)
│   ├── adf.rs           # ADF validation & text→ADF conversion
│   └── fields.rs        # Field optimization (17 defaults)
└── confluence/
    ├── api.rs           # 6 operations (search, get, create, update, children, comments)
    └── fields.rs        # Field filtering for v1/v2 APIs
```

---

## Architecture

### Data Flow

```
Terminal
  ↓ clap::Parser
CLI Command (main.rs)
  ↓ match subcommand
Load Config (4-tier priority)
  ↓
API Operation (jira/api.rs or confluence/api.rs)
  ↓ field filtering
HTTP Request (reqwest)
  ↓
JSON Response
  ↓ stdout
```

**Key Points**:
- No response filtering applied (filter.rs is dead code)
- Field filtering: 17 defaults for Jira search (60-70% reduction)
- ADF: Auto-converts plain text → JSON ADF format
- Config: Cached base_url at initialization

### Configuration Priority

```
1. CLI Flags (--domain, --email, --token)
2. Environment Variables (ATLASSIAN_*)
3. Project Config (./.atlassian.toml)
4. Global Config (~/.config/atlassian-cli/config.toml)
```

---

## Key Patterns

### 4-Tier Config Loading

```rust
// config.rs
pub fn load(
    config_path: Option<&PathBuf>,
    profile: Option<&String>,
    domain: Option<&String>,    // CLI flag
    email: Option<&String>,     // CLI flag
    token: Option<&String>,     // CLI flag
) -> Result<Self>

// Priority: CLI > ENV > File > Default
let domain = domain.cloned()
    .or_else(|| env::var("ATLASSIAN_DOMAIN").ok())
    .or_else(|| config_from_file.domain)
    .ok_or_else(|| anyhow!("domain required"))?;
```

### Field Optimization (60-70% Reduction)

**Default 17 fields** (jira/fields.rs):
```rust
const DEFAULT_SEARCH_FIELDS: &[&str] = &[
    "key", "summary", "status", "priority", "issuetype",
    "assignee", "reporter", "creator",
    "created", "updated", "duedate", "resolutiondate",
    "project", "labels", "components", "parent", "subtasks",
];
```

**Why 17?**
- Excludes `description` (large text field, 10s of KB)
- Excludes `id` (redundant with `key`)
- Excludes `renderedFields` (HTML rendering)

**Priority Hierarchy**:
```rust
fn resolve_search_fields(
    api_fields: Option<Vec<String>>,  // 1. Highest priority
    config: &Config,
) -> Vec<String> {
    // 2. JIRA_SEARCH_DEFAULT_FIELDS env (replaces defaults)
    // 3. DEFAULT_SEARCH_FIELDS + JIRA_SEARCH_CUSTOM_FIELDS
    // 4. DEFAULT_SEARCH_FIELDS only
}
```

### ADF Auto-Conversion

```rust
// jira/adf.rs
pub fn process_adf_input(value: Value) -> Result<Value> {
    match value {
        Value::String(text) => Ok(text_to_adf(&text)),  // Auto-convert
        Value::Object(_) => {
            validate_adf(&value)?;  // Validate if JSON
            Ok(value)
        }
        _ => anyhow::bail!("must be string or ADF object")
    }
}

fn text_to_adf(text: &str) -> Value {
    json!({
        "type": "doc",
        "version": 1,
        "content": [{
            "type": "paragraph",
            "content": [{"type": "text", "text": text}]
        }]
    })
}
```

**Validation rules** (top-level only):
- `type` must be "doc"
- `version` must be 1
- `content` must be array

### Zero-Copy Pattern

```rust
// Extract value without cloning
let description = args.get_mut("description")
    .map(|v| std::mem::replace(v, Value::Null))
    .unwrap_or(Value::Null);
```

---

## Module Quick Ref

### config.rs

**Key fields**:
- `domain`, `email`, `token`: Required auth
- `base_url: String`: Cached `https://{domain}` (computed once)
- `jira_search_default_fields`: Override 17 defaults
- `jira_search_custom_fields`: Extend defaults
- `projects_filter`, `spaces_filter`: Access control

**Functions**:
- `load()`: 4-tier priority loading
- `load_without_validation()`: For `config show` command
- `validate()`: Domain/email format checks
- `normalize_base_url()`: Ensures https:// prefix

### jira/api.rs (8 operations)

| Function | Endpoint | Notes |
|----------|----------|-------|
| `get_issue` | GET `/rest/api/3/issue/{key}` | Field filtering |
| `search` | GET `/rest/api/3/search` | 17-field optimization, JQL |
| `create_issue` | POST `/rest/api/3/issue` | ADF support |
| `update_issue` | PUT `/rest/api/3/issue/{key}` | ADF support |
| `add_comment` | POST `/rest/api/3/issue/{key}/comment` | ADF support |
| `update_comment` | PUT `/rest/api/3/issue/{key}/comment/{id}` | ADF support |
| `get_transitions` | GET `/rest/api/3/issue/{key}/transitions` | Lists states |
| `transition_issue` | POST `/rest/api/3/issue/{key}/transitions` | Change state |

**Response format** (optimized):
- `search`: `{"items": [...], "total": N}` (not "issues")
- `create_issue`: `{"key": "...", "id": "..."}` (not "success")
- `update_issue`: `{}` (empty = success)

### jira/adf.rs

**Key functions**:
- `text_to_adf(text: &str) -> Value`: Plain text → ADF
- `process_adf_input(value: Value) -> Result<Value>`: Validates or converts
- `validate_adf(doc: &Value) -> Result<()>`: Top-level validation only

**Supported in description/comment**: String or ADF JSON object

### jira/fields.rs

**Constants**:
- `DEFAULT_SEARCH_FIELDS`: 17 fields array
- Used for: `jira search` command

**Functions**:
- `resolve_search_fields()`: Priority hierarchy
- `apply_field_filtering_to_url()`: Removes `expand=renderedFields`

### confluence/api.rs (6 operations)

| Function | Endpoint | API Version |
|----------|----------|-------------|
| `search` | `/wiki/rest/api/content/search` | v1 (CQL) |
| `get_page` | `/wiki/api/v2/pages/{id}` | v2 |
| `get_page_children` | `/wiki/api/v2/pages/{id}/children` | v2 |
| `get_comments` | `/wiki/api/v2/pages/{id}/footer-comments` | v2 |
| `create_page` | `/wiki/api/v2/pages` | v2 |
| `update_page` | `/wiki/api/v2/pages/{id}` | v2 (version handling) |

**Version handling**: CLI auto-increments version (no manual input needed)

### confluence/fields.rs

**Functions**:
- `apply_expand_filtering()`: For v1 search API
- `apply_v2_filtering()`: For v2 APIs

**Env var**: `CONFLUENCE_CUSTOM_INCLUDES` (ancestors, children, history, operations, labels, properties)

---

## Development Tasks

### Add New Jira Command

1. **main.rs**: Add to `JiraSubcommand` enum
   ```rust
   enum JiraSubcommand {
       // ...
       NewCommand { param: String },
   }
   ```

2. **main.rs**: Add handler
   ```rust
   JiraSubcommand::NewCommand { param } => {
       let result = jira::new_command(&param, &config).await?;
       println!("{}", serde_json::to_string_pretty(&result)?);
   }
   ```

3. **jira/api.rs**: Implement function
   ```rust
   pub async fn new_command(param: &str, config: &Config) -> Result<Value> {
       let url = format!("{}/rest/api/3/endpoint", config.base_url());
       let client = http::create_client(config)?;
       let response = client.get(&url).send().await?;
       Ok(response.json().await?)
   }
   ```

4. **Test**: Add test in `jira/api.rs`

### Modify Field Filtering

1. **jira/fields.rs**: Update `DEFAULT_SEARCH_FIELDS`
2. **Test impact**: Check test fixtures
3. **Update docs**: README.md field count

### Add ADF Support to New Field

1. **Extract value**: Use `std::mem::replace()` pattern
2. **Process**: Call `adf::process_adf_input()`
3. **Insert**: Add to request body

---

## Common Issues

### Config Not Found

**Symptom**: `ATLASSIAN_API_TOKEN not configured`

**Check**:
```bash
atlassian config show
atlassian config path
```

**Fix**: Check 4-tier priority, ensure token in config or env

### Field Filtering Not Working

**Check priority**:
1. CLI `--fields` parameter (highest)
2. `JIRA_SEARCH_DEFAULT_FIELDS` env
3. Defaults + `JIRA_SEARCH_CUSTOM_FIELDS`

**Debug**:
```bash
JIRA_SEARCH_DEFAULT_FIELDS="key,summary" atlassian jira search "..."
```

### Tests Failing

**Run with output**:
```bash
cargo test -- --nocapture
cargo test jira::adf  # Specific module
```

### Binary Size Too Large

**Check profile**:
```toml
[profile.release]
opt-level = 3
lto = true
codegen-units = 1
strip = true
```

Expected: 3.8MB

---

## Testing Strategy

**Test breakdown** (120 total):
```
config.rs:       13 tests  # Config loading, priority, validation
http.rs:         3 tests   # Client creation, auth
filter.rs:       3 tests   # Dead code (not used in CLI)
test_utils.rs:   3 tests   # Helper functions
jira/adf.rs:     35 tests  # ADF validation, conversion
jira/api.rs:     27 tests  # 8 operations
jira/fields.rs:  18 tests  # Field resolution, priority
confluence/api.rs: 18 tests # 6 operations
```

**Run tests**:
```bash
cargo test                  # All tests
cargo test jira::adf       # Module tests
cargo test -- --nocapture  # With output
```

---

## Performance Characteristics

| Metric | Value | Notes |
|--------|-------|-------|
| Binary size | 3.8MB | LTO + strip enabled |
| Build time (release) | ~30s | Single codegen unit |
| Test time | < 1s | 120 tests |
| Field optimization | 60-70% | 17 vs 50+ fields |
| API call overhead | ~500ms | Network dependent |

---

## Configuration Reference

See [README.md](README.md) for user-facing config docs.

**Internal constants**:
```rust
// jira/fields.rs
DEFAULT_SEARCH_FIELDS: 17 fields

// config.rs defaults
base_url: String (cached)
normalize_base_url(): ensures https://
```

**Environment variables**:
```bash
ATLASSIAN_DOMAIN=company.atlassian.net
ATLASSIAN_EMAIL=user@example.com
ATLASSIAN_API_TOKEN=token

JIRA_SEARCH_DEFAULT_FIELDS=key,summary,status
JIRA_SEARCH_CUSTOM_FIELDS=customfield_10015
CONFLUENCE_CUSTOM_INCLUDES=ancestors,history
```

---

## Build & Release

```bash
cargo build                # Development
cargo build --release      # Optimized (3.8MB)
cargo test                 # 120 tests
cargo clippy -- -D warnings # Zero warnings policy
cargo fmt                  # Format
```

**Cargo.toml optimizations**:
```toml
[profile.release]
opt-level = 3       # Maximum optimization
lto = true          # Link-time optimization
codegen-units = 1   # Single unit for better optimization
strip = true        # Remove debug symbols
```

---

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| clap | 4.5 | CLI parsing (derive API) |
| tokio | 1.48 | Async runtime |
| reqwest | 0.12 | HTTP client (rustls, no OpenSSL) |
| serde_json | 1.0 | JSON serialization |
| anyhow | 1.0 | Error handling |
| toml | 0.9 | Config file parsing |
| dirs | 6.0 | Platform-specific paths |

**No unused dependencies**. All features justified.

---

## Resources

- [README.md](README.md) - User guide
- [Jira REST API v3](https://developer.atlassian.com/cloud/jira/platform/rest/v3/)
- [Confluence REST API v2](https://developer.atlassian.com/cloud/confluence/rest/v2/)
- [ADF Specification](https://developer.atlassian.com/cloud/jira/platform/apis/document/structure/)

---

**This guide is optimized for AI agents: project-specific knowledge only, no general Rust/HTTP/JSON concepts.**

**Version**: 0.1.0
**Tests**: 120 passing
**Production-ready**: Yes
