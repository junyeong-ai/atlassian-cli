# Atlassian CLI - AI Agent Developer Guide

Essential knowledge for implementing features and debugging this Rust CLI tool.

---

## Core Patterns

### 4-Tier Config Priority

**Implementation**:
```rust
pub fn load(
    config_path: Option<&PathBuf>,
    profile: Option<&String>,
    domain: Option<&String>,    // CLI flag
    email: Option<&String>,     // CLI flag
    token: Option<&String>,     // CLI flag
) -> Result<Self>

// Priority chain
let domain = domain.cloned()
    .or_else(|| env::var("ATLASSIAN_DOMAIN").ok())
    .or_else(|| config_from_file.domain)
    .ok_or_else(|| anyhow!("domain required"))?;
```

**Why**: Flexible configuration for different environments without code changes.

**Locations**:
- CLI: `--domain`, `--email`, `--token` flags
- ENV: `ATLASSIAN_DOMAIN`, `ATLASSIAN_EMAIL`, `ATLASSIAN_API_TOKEN`
- Project: `./.atlassian.toml`
- Global: `~/.config/atlassian-cli/config.toml`

---

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

**Excluded**: `description` (large text), `id` (redundant), `renderedFields` (HTML)

**Priority Hierarchy**:
1. CLI `--fields` (highest)
2. `JIRA_SEARCH_DEFAULT_FIELDS` env (replaces defaults)
3. DEFAULT_SEARCH_FIELDS + `JIRA_SEARCH_CUSTOM_FIELDS` env

**Why**: Jira responses can be 10s of KB with `description` field. 17 defaults reduce by 60-70%.

---

### ADF Auto-Conversion

**Pattern**:
```rust
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

**Why**: Users can provide plain text, CLI auto-converts to ADF for Jira/Confluence.

---

### Zero-Copy Pattern

**Usage**:
```rust
// Extract value without cloning
let description = args.get_mut("description")
    .map(|v| std::mem::replace(v, Value::Null))
    .unwrap_or(Value::Null);
```

**Why**: Avoids cloning large strings. Used for ADF processing of description fields.

---

## Development Tasks

### Add Jira Command

1. **main.rs**: Add to `JiraSubcommand` enum
   ```rust
   enum JiraSubcommand {
       // ...
       NewCommand { param: String },
   }
   ```

2. **main.rs**: Add handler in match block
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

---

### Modify Field Filtering

1. **jira/fields.rs**: Update `DEFAULT_SEARCH_FIELDS`
2. **Test impact**: Check test fixtures
3. **Update docs**: README.md field count

---

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
atlassian-cli config show
atlassian-cli config path
```

**Fix**: Ensure token in config file or environment variable

---

### Field Filtering Not Working

**Check priority**:
1. CLI `--fields` parameter (highest)
2. `JIRA_SEARCH_DEFAULT_FIELDS` env
3. Defaults + `JIRA_SEARCH_CUSTOM_FIELDS`

**Debug**:
```bash
JIRA_SEARCH_DEFAULT_FIELDS="key,summary"  atlassian-cli jira search "..."
```

---

### Project Filter Injection

**Symptom**: JQL doesn't filter by project

**Cause**: `projects_filter` config auto-injects project clause

**Behavior**:
```
Input:  status = Open
Output: project IN (PROJ1,PROJ2) AND (status = Open)
```

**Fix**: Remove `projects_filter` if not desired, or add explicit project to JQL

---

## Key Constants

**Locations**:
- `jira/fields.rs`: DEFAULT_SEARCH_FIELDS (17 fields)
- `config.rs`: Default timeouts, TTL values
- `jira/api.rs`: API endpoints
- `confluence/api.rs`: API endpoints (v1/v2)

**To modify**: Edit constant in source, or add to `Config` struct + `config.toml` for user configuration.

---

This guide contains only implementation-critical knowledge. For user documentation, see [README.md](README.md).
