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
    ├── fields.rs    # v2 include params, v1 expand params
    └── markdown.rs  # HTML→Markdown conversion (htmd)
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

### ADF Conversion
```rust
// jira/adf.rs - bidirectional ADF ↔ Markdown/Text conversion
// Input: plain text → ADF JSON (for create/update)
pub fn process_adf_input(value: Value) -> Result<Value> {
    match value {
        Value::String(text) => Ok(text_to_adf(&text)),
        Value::Object(_) => { validate_adf(&value)?; Ok(value) }
        _ => bail!("must be string or ADF")
    }
}

// Output: ADF → Markdown (for reading)
pub fn adf_to_markdown(adf: &Value) -> String {
    // Handles: paragraph, heading, bulletList, orderedList,
    // codeBlock, panel, table, mentions, links, marks
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

### HTML→Markdown Conversion
```rust
// confluence/markdown.rs - Confluence HTML to Markdown
pub fn convert_to_markdown(html: &str) -> String {
    let converter = HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style"])
        .build();
    // Handles ac:*, ri:* tags, images, macros
    let cleaned = clean_confluence_html(html);
    converter.convert(&cleaned)...
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

## Constants & Configurable Values

| Location | Constant/Config | Default | Configurable |
|----------|----------------|---------|--------------|
| `jira/fields.rs` | `DEFAULT_SEARCH_FIELDS` | 17 fields | ✅ `[jira]` |
| `confluence/api.rs` | `MAX_LIMIT` | 250 | ❌ API limit |
| `confluence/api.rs` | `SEARCH_BODY_LIMIT` | 50 | ❌ API limit |
| `config.rs` | `request_timeout_ms` | 30000 | ✅ `[performance]` |
| `config.rs` | `rate_limit_delay_ms` | 200 | ✅ `[performance]` |

## API Endpoints

- Jira: `/rest/api/3/*`
- Confluence Search: `/wiki/rest/api/search` (v1, uses `expand` param)
- Confluence Pages: `/wiki/api/v2/pages/*` (v2, uses `include-*` params)

## CLI Options (Jira)

| Option | Description | Applies To |
|--------|-------------|------------|
| `--format` | Output format: `html` (default, raw ADF) or `markdown` | get, search |
| `--fields` | Specify fields to return | search |
| `--limit N` | Max results (default: 20) | search |

## CLI Options (Confluence)

| Option | Description | Applies To |
|--------|-------------|------------|
| `--limit N` | Max results per request (default: 10, max: 250) | search |
| `--all` | Fetch all results via cursor pagination | search |
| `--stream` | Output JSONL (requires --all) | search |
| `--expand` | Additional expand fields: `ancestors`, `space`, etc. (body.storage included by default) | search |
| `--format` | Output format: `html` (default) or `markdown` | search, get, comments |

Note: `children` command does not support `--format` (v2 API limitation).

## Testing

```bash
cargo test                    # all tests
cargo test confluence         # module tests
cargo clippy && cargo fmt     # lint
```
