# Atlassian CLI - AI Agent Guide

## Project Structure

```
src/
├── main.rs          # CLI parsing (clap), command handlers
├── config.rs        # 4-tier config: CLI > ENV > project(.atlassian.toml) > global
├── http.rs          # reqwest client, auth header
├── filter.rs        # Response field filtering
├── jira/
│   ├── api.rs       # Jira REST API v3
│   ├── fields.rs    # DEFAULT_SEARCH_FIELDS (17 fields)
│   └── adf.rs       # Atlassian Document Format conversion
└── confluence/
    ├── api.rs       # Confluence REST API v1/v2, pagination
    └── fields.rs    # v2 include params, v1 expand params
```

## Key Patterns

### Config Priority Chain
```rust
// config.rs - CLI flag > ENV > file
let domain = cli_domain
    .or_else(|| env::var("ATLASSIAN_DOMAIN").ok())
    .or_else(|| file_config.domain);
```

### Field Optimization
```rust
// jira/fields.rs - excludes description, id, renderedFields
const DEFAULT_SEARCH_FIELDS: &[&str] = &[
    "key", "summary", "status", "priority", "issuetype",
    "assignee", "reporter", "creator", "created", "updated",
    "duedate", "resolutiondate", "project", "labels",
    "components", "parent", "subtasks",
];
```

### ADF Auto-Conversion
```rust
// jira/adf.rs - plain text → ADF JSON
pub fn process_adf_input(value: Value) -> Result<Value> {
    match value {
        Value::String(text) => Ok(text_to_adf(&text)),
        Value::Object(_) => { validate_adf(&value)?; Ok(value) }
        _ => bail!("must be string or ADF")
    }
}
```

### Cursor-Based Pagination
```rust
// confluence/api.rs - search_all uses _links.next
loop {
    let data = fetch_page(&client, &url, config).await?;
    all_items.extend(data["results"].as_array());

    match data["_links"]["next"].as_str() {
        Some(next) => url = build_next_url(base_url, next),
        None => break,
    }
    sleep(Duration::from_millis(200)).await; // rate limit
}
```

### Auto-Injected Filters
```rust
// JQL: projects_filter adds "project IN (...) AND"
// CQL: spaces_filter adds "space IN (...) AND"
fn apply_space_filter(cql: &str, config: &Config) -> String {
    if cql.to_lowercase().contains("space ") {
        cql.to_string()  // user specified, skip injection
    } else {
        format!("space IN ({}) AND ({})", spaces, cql)
    }
}
```

## Adding Commands

1. **main.rs**: Add variant to `JiraSubcommand` or `ConfluenceSubcommand`
2. **main.rs**: Add match arm in `handle_jira` or `handle_confluence`
3. **api.rs**: Implement async function

## Constants

| Location | Constant | Value |
|----------|----------|-------|
| `jira/fields.rs` | `DEFAULT_SEARCH_FIELDS` | 17 fields |
| `confluence/api.rs` | `MAX_LIMIT` | 250 |
| `confluence/api.rs` | `RATE_LIMIT_DELAY_MS` | 200 |
| `config.rs` | `default_timeout` | 30000ms |

## API Endpoints

- Jira: `/rest/api/3/*`
- Confluence Search: `/wiki/rest/api/search` (v1, uses `expand` param)
- Confluence Pages: `/wiki/api/v2/pages/*` (v2, uses `include-*` params)

## CLI Options (Confluence Search)

| Option | Description |
|--------|-------------|
| `--limit N` | Max results per request (default: 10, max: 250) |
| `--all` | Fetch all results via cursor pagination |
| `--stream` | Output JSONL (requires --all) |
| `--expand` | Expand fields: `body.storage`, `ancestors`, `version`, etc. |

## Testing

```bash
cargo test                    # all tests
cargo test confluence         # module tests
cargo clippy && cargo fmt     # lint
```
