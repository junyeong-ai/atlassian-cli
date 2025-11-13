# Atlassian CLI Development Guide

**Rust CLI for Atlassian Cloud (Jira & Confluence)**

---

## Project Overview

Production-ready CLI tool implementing 14 operations for Jira and Confluence.

| Metric | Value |
|--------|-------|
| **Language** | Rust 2024 Edition (MSRV 1.91.1) |
| **Binary** | 3.8MB (release, stripped) |
| **Operations** | 14 (8 Jira + 6 Confluence) |
| **Tests** | 122 passing (120 unit + 2 doc) |
| **Warnings** | Zero (strict clippy policy) |

### Technology Stack

```toml
tokio = "1.48"              # Async runtime
reqwest = "0.12.24"         # HTTP client (rustls-tls)
clap = "4.5.51"             # CLI parsing
serde_json = "1.0.145"      # JSON processing
anyhow = "1.0.100"          # Error handling
```

### Build Profile

```toml
[profile.release]
opt-level = 3               # Maximum optimization
lto = true                  # Link-time optimization
codegen-units = 1           # Single codegen unit
strip = true                # Strip debug symbols
```

---

## Architecture

### Module Structure

```
src/
├── main.rs                 # Clap CLI entry point
├── lib.rs                  # Library exports
├── config.rs               # Config loading (4-tier priority)
├── http.rs                 # HTTP client factory
├── filter.rs               # Response optimization
├── test_utils.rs           # Shared test helpers
├── jira/
│   ├── mod.rs              # Module exports
│   ├── api.rs              # 8 Jira functions
│   ├── adf.rs              # ADF processing (zero-copy)
│   └── fields.rs           # Field filtering
└── confluence/
    ├── mod.rs              # Module exports
    ├── api.rs              # 6 Confluence functions
    └── fields.rs           # Field optimization
```

### Key Design Patterns

1. **Zero-Copy Optimizations**
   - `Config.base_url: String` cached at initialization
   - ADF processing uses move semantics (`std::mem::replace`)
   - Response filtering modifies in-place

2. **Configuration Priority** (4 tiers)
   - CLI flags > Env vars > Project config > Global config
   - See `config.rs::Config::load()`

3. **Field Filtering** (token optimization)
   - Jira search: 17 default fields (excludes description)
   - Response optimizer: removes 25 unnecessary fields
   - Priority: API params > env override > defaults + custom

4. **Error Handling**
   - `anyhow::Result` throughout
   - HTTP errors surfaced with context
   - Config validation at startup

---

## Core Modules

### `main.rs` - CLI Entry Point

**Clap structure**:
```rust
Command::Jira(JiraCommand)
  - Get, Search, Create, Update, Transitions, Transition, Comment
Command::Confluence(ConfluenceCommand)
  - Search, Get, Create, Update, Children, Comments
Command::Config(ConfigCommand)
  - Init, Show, List, Path, Edit
```

**Flow**: Parse CLI → Load config → Execute operation → Output JSON

### `config.rs` - Configuration Management

**Config priority**:
1. CLI flags (`--domain`, `--email`, `--token`)
2. Environment variables (`ATLASSIAN_*`)
3. Project config (`./.atlassian.toml`)
4. Global config (`~/.config/atlassian-cli/config.toml`)

**Key fields**:
- `domain`, `email`, `token`: Required for API auth
- `base_url: String`: Cached `https://{domain}` (computed once)
- `jira_search_default_fields`: Override default 17 fields
- `jira_search_custom_fields`: Extend defaults
- `response_exclude_fields`: Token optimization (25 default)
- `projects_filter`, `spaces_filter`: Access control

**Validation**: Domain must contain `.atlassian.net`, email must contain `@`

### `jira/api.rs` - Jira Operations (8)

| Function | Endpoint | Notes |
|----------|----------|-------|
| `get_issue` | GET `/rest/api/3/issue/{key}` | Field filtering |
| `search` | GET `/rest/api/3/search` | 17-field optimization, JQL |
| `create_issue` | POST `/rest/api/3/issue` | ADF support (description) |
| `update_issue` | PUT `/rest/api/3/issue/{key}` | ADF support (description) |
| `add_comment` | POST `/rest/api/3/issue/{key}/comment` | ADF support (comment) |
| `update_comment` | PUT `/rest/api/3/issue/{key}/comment/{id}` | ADF support (body) |
| `get_transitions` | GET `/rest/api/3/issue/{key}/transitions` | Lists available states |
| `transition_issue` | POST `/rest/api/3/issue/{key}/transitions` | Change workflow state |

**Response format** (optimized):
- `get_issue`: Direct object return (no wrapping)
- `search`: `{"items": [...], "total": N}` (not "issues")
- `create_issue`: `{"key": "...", "id": "..."}` (no "success")
- `update_issue`: `{}` (empty response)
- `add_comment`: `{"id": "..."}` (not "comment_id")
- `get_transitions`: Direct array return
- `transition_issue`: `{}` (empty response)

### `jira/adf.rs` - ADF Processing

**ADF (Atlassian Document Format)**: JSON-based rich text format for Jira.

**Key functions**:
- `text_to_adf(text: &str) -> Value`: Converts plain text to ADF
- `process_adf_input(value: Value) -> Result<Value>`: Validates or converts
- `validate_adf(doc: &Value) -> Result<()>`: Top-level validation only

**Validation rules**:
- `type` must be "doc"
- `version` must be 1
- `content` must be array

**Optimization**: Uses move semantics (no clone) for large documents.

### `jira/fields.rs` - Field Filtering

**Default 17 fields**:
```rust
const DEFAULT_SEARCH_FIELDS: &[&str] = &[
    "key", "summary", "status", "priority", "issuetype",
    "assignee", "reporter", "creator",
    "created", "updated", "duedate", "resolutiondate",
    "project", "labels", "components", "parent", "subtasks",
];
```

**Resolution logic**:
```rust
fn resolve_search_fields(
    api_fields: Option<Vec<String>>,
    config: &Config,
) -> Vec<String>
```

Priority: API params > `JIRA_SEARCH_DEFAULT_FIELDS` env > defaults + custom

**Why 17 fields?**
- Excludes `description` (large text field, 10s of KB)
- Excludes `id` (redundant with `key`)
- 60-70% size reduction vs default response

### `confluence/api.rs` - Confluence Operations (6)

| Function | Endpoint | Notes |
|----------|----------|-------|
| `search` | GET `/wiki/rest/api/content/search` | CQL, v1 API |
| `get_page` | GET `/wiki/api/v2/pages/{id}` | v2 API |
| `get_page_children` | GET `/wiki/api/v2/pages/{id}/children` | v2 API |
| `get_comments` | GET `/wiki/api/v2/pages/{id}/footer-comments` | v2 API |
| `create_page` | POST `/wiki/api/v2/pages` | v2 API |
| `update_page` | PUT `/wiki/api/v2/pages/{id}` | Version handling, v2 API |

**Response format** (optimized):
- `search`: `{"items": [...], "total": N}` (not "results")
- `get_page`: Direct object return
- `get_page_children`: `{"items": [...]}` (not "children")
- `get_comments`: `{"items": [...]}` (not "comments")
- `create_page`: `{"id": "...", "title": "..."}` (not "page_id")
- `update_page`: `{"id": "...", "version": {...}}` (not "page_id")

### `filter.rs` - Response Optimization

**ResponseOptimizer**: Removes unnecessary fields for token efficiency.

**Default excluded fields (25)**:
```rust
// UI metadata
avatarUrls, iconUrl, profilePicture, icon, avatarId,
colorName, iconCssClass

// API metadata
expand, _expandable, self

// Fixed values
accountType, projectTypeKey, simplified, entityType

// Empty objects
childTypes, macroRenderedOutput, restrictions, breadcrumbs

// Workflow metadata
hasScreen, isAvailable, isConditional, isGlobal,
isInitial, isLooped

// Duplicates
friendlyLastModified
```

**Result**: 20-30% token reduction.

**Usage**: Applied after all API responses automatically.

### `http.rs` - HTTP Client

**Authentication**: HTTP Basic Auth (Base64-encoded `email:token`)

**Client configuration**:
- TLS: rustls (not OpenSSL)
- Timeout: configurable (`REQUEST_TIMEOUT_MS`, default 30000ms)
- User-Agent: `atlassian-cli/{version}`

**Error handling**: HTTP errors wrapped with context (status code, URL)

### `test_utils.rs` - Test Helpers

**Purpose**: Shared test utilities to avoid duplication across 6 test files.

**Functions**:
- `create_test_config()`: Default test config
- `create_test_config_with_fields(...)`: Custom field config
- `create_test_config_with_filters(...)`: Access control config

---

## Testing

### Test Structure (122 tests)

```
config.rs:       13 tests  # Config loading, priority, validation
http.rs:         3 tests   # Client creation, auth
filter.rs:       3 tests   # Response filtering
test_utils.rs:   3 tests   # Helper functions
jira/adf.rs:     35 tests  # ADF validation, conversion
jira/api.rs:     27 tests  # 8 API operations
jira/fields.rs:  18 tests  # Field resolution, priority
confluence/api.rs: 18 tests # 6 API operations
---
Total:           120 unit tests
Doctests:        2 (jira/adf.rs)
```

### Running Tests

```bash
# All tests
cargo test

# Specific module
cargo test jira::adf
cargo test config

# With output
cargo test -- --nocapture
```

---

## Development Workflow

### Build

```bash
# Development
cargo build

# Release (optimized, 3.8MB)
cargo build --release

# Check only (fast)
cargo check
```

### Code Quality

```bash
# Format (required)
cargo fmt

# Lint (zero warnings policy)
cargo clippy --all-targets -- -D warnings

# Full check
cargo fmt && cargo clippy && cargo test
```

### Adding New Operations

1. **Add function to `jira/api.rs` or `confluence/api.rs`**:
   ```rust
   pub async fn new_operation(
       param: &str,
       config: &Config,
   ) -> Result<Value> {
       // Implementation
   }
   ```

2. **Add CLI command to `main.rs`**:
   ```rust
   enum JiraSubcommand {
       // ...
       NewOperation { param: String },
   }
   ```

3. **Add tests**: Follow existing test patterns

4. **Update field filtering** (if needed): Modify `jira/fields.rs` or `confluence/fields.rs`

---

## Common Patterns

### Error Handling

```rust
use anyhow::{Context, Result};

// Add context to errors
response.json().await
    .context("Failed to parse JSON response")?
```

### Config Access

```rust
// Base URL (cached)
let base_url = config.base_url.as_str();

// Or use convenience method
let base_url = config.base_url();
```

### Zero-Copy Pattern

```rust
// Extract value without cloning
let description = args.get_mut("description")
    .map(|v| std::mem::replace(v, Value::Null))
    .unwrap_or(Value::Null);
```

### Field Filtering

```rust
// Resolve fields with priority
let fields = jira::fields::resolve_search_fields(
    api_fields,
    config,
);

// Add to query params
params.push(("fields", fields.join(",")));
```

---

## Performance Considerations

1. **Config loading**: Base URL cached at initialization (avoid repeated String allocation)

2. **ADF processing**: Move semantics for large documents (10s of KB)

3. **Field filtering**: 17 fields vs 50+ fields (60-70% reduction)

4. **Response optimization**: 25 fields removed (20-30% token reduction)

5. **HTTP client**: Reused across requests (connection pooling)

---

## Security

### Authentication
- HTTP Basic Auth (Base64-encoded)
- Token stored in config files (permissions: 600)
- HTTPS only (enforced)

### Input Validation
- Config validation at startup
- JQL/CQL passed directly to Atlassian API (server-side validation)
- JSON parsing errors handled

### Access Control
- `projects_filter`: Restricts Jira projects
- `spaces_filter`: Restricts Confluence spaces
- Auto-injection if user doesn't specify project/space

---

## Troubleshooting

### Common Issues

1. **Config not found**:
   - Check config priority (CLI > env > project > global)
   - Use `atlassian config show` to verify

2. **API errors**:
   - Verify credentials: `ATLASSIAN_DOMAIN`, `ATLASSIAN_EMAIL`, `ATLASSIAN_API_TOKEN`
   - Check domain format: `company.atlassian.net` (no `https://`)

3. **Field filtering not working**:
   - Check priority: API params > env > defaults
   - Verify env var: `JIRA_SEARCH_DEFAULT_FIELDS`

4. **Tests failing**:
   - Run `cargo test -- --nocapture` for detailed output
   - Check if test_utils.rs changes affected tests

---

## Resources

### Atlassian APIs
- [Jira REST API v3](https://developer.atlassian.com/cloud/jira/platform/rest/v3/)
- [Confluence REST API v2](https://developer.atlassian.com/cloud/confluence/rest/v2/)
- [ADF Specification](https://developer.atlassian.com/cloud/jira/platform/apis/document/structure/)

### Rust
- [Tokio Docs](https://docs.rs/tokio)
- [Reqwest Docs](https://docs.rs/reqwest)
- [Clap Docs](https://docs.rs/clap)

---

## License

MIT

---

**Optimized for AI agent comprehension - focuses on architecture, patterns, and maintenance workflow**
