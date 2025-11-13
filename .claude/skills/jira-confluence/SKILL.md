---
name: jira-confluence
description: Atlassian CLI for Jira issues and Confluence wiki pages. Use when querying issues with JQL, searching pages with CQL, managing sprint workflows, creating/updating tickets or pages, handling ADF rich text, or automating bulk operations. Supports 14 operations across Jira and Confluence Cloud APIs.
allowed-tools: [Bash, Read]
---

# Jira & Confluence CLI Expert Guide

Expert guide for the `atlassian` CLI tool for Jira and Confluence Cloud APIs.

## Authentication

**4-tier priority** (highest to lowest):
1. **CLI flags**: `--domain company.atlassian.net --email user@example.com --token TOKEN`
2. **Environment variables**: `ATLASSIAN_DOMAIN`, `ATLASSIAN_EMAIL`, `ATLASSIAN_API_TOKEN`
3. **Project config**: `./.atlassian.toml`
4. **Global config**: `~/.config/atlassian-cli/config.toml`

**Domain format**: `company.atlassian.net` (NOT `https://company.atlassian.net`)

**Multi-tenant**: Use `--profile work` for multiple Atlassian instances.

## Jira Operations (8 commands)

| Operation | Syntax | Returns |
|-----------|--------|---------|
| **Get issue** | `atlassian jira get PROJ-123` | Direct issue object |
| **Search** | `atlassian jira search "<JQL>" --limit N --fields a,b` | `{"items": [...], "total": N}` |
| **Create** | `atlassian jira create PROJ "Title" Bug --description "text"` | `{"key": "PROJ-123", "id": "..."}` |
| **Update** | `atlassian jira update PROJ-123 '{"summary":"New"}'` | `{}` (empty = success) |
| **Comment add** | `atlassian jira comment add PROJ-123 "Comment text"` | `{"id": "..."}` |
| **Comment update** | `atlassian jira comment update PROJ-123 <ID> "New text"` | `{"id": "..."}` |
| **List transitions** | `atlassian jira transitions PROJ-123` | Array of `{id, name, to: {name}}` |
| **Execute transition** | `atlassian jira transition PROJ-123 <ID>` | `{}` (empty = success) |

**ADF Support**: `--description` and comment text accept plain text (auto-converted to ADF) or JSON ADF object.

## Confluence Operations (6 commands)

| Operation | Syntax | Returns |
|-----------|--------|---------|
| **Search** | `atlassian confluence search "<CQL>" --limit N` | `{"items": [...], "total": N}` |
| **Get page** | `atlassian confluence get <PAGE_ID>` | Direct page object |
| **List children** | `atlassian confluence children <PAGE_ID>` | `{"items": [...]}` |
| **Get comments** | `atlassian confluence comments <PAGE_ID>` | `{"items": [...]}` |
| **Create page** | `atlassian confluence create <SPACE_KEY> "Title" "<html>"` | `{"id": "...", "title": "..."}` |
| **Update page** | `atlassian confluence update <PAGE_ID> "Title" "<html>"` | `{"id": "...", "version": {...}}` |

**Important**:
- **Space key**: Use space KEY (e.g., "TEAM"), not ID. CLI auto-converts.
- **Content format**: HTML storage format (NOT markdown): `"<p>Content with <strong>bold</strong></p>"`
- **Version handling**: CLI auto-increments version (no manual version needed).
- **Field includes**: `CONFLUENCE_CUSTOM_INCLUDES=ancestors,history` (valid: ancestors, children, history, operations, labels, properties)

## ADF (Atlassian Document Format)

**Plain text** (recommended): CLI auto-converts to ADF.
```bash
atlassian jira create PROJ "Title" Bug --description "Plain text description"
```

**JSON ADF** (advanced):
```bash
atlassian jira create PROJ "Title" Bug --description '{
  "type": "doc",
  "version": 1,
  "content": [
    {"type": "paragraph", "content": [{"type": "text", "text": "Rich text"}]}
  ]
}'
```

## Field Optimization (Jira Search)

**3-tier priority**:
1. **CLI `--fields`**: `--fields key,summary,status` (per-request override)
2. **JIRA_SEARCH_DEFAULT_FIELDS**: Replaces all defaults
3. **Defaults + JIRA_SEARCH_CUSTOM_FIELDS**: Extends 17 defaults

**Default 17 fields**: key, summary, status, priority, issuetype, assignee, reporter, creator, created, updated, duedate, resolutiondate, project, labels, components, parent, subtasks

**Excluded**: `description` (large text field, 10s of KB)

**Result**: 60-70% size reduction vs full response.

## Project/Space Auto-Injection

**Config**:
```toml
[default.jira]
projects_filter = ["PROJ1", "PROJ2"]

[default.confluence]
spaces_filter = ["SPACE1"]
```

**Effect**: JQL becomes `project IN (PROJ1,PROJ2) AND (your_jql)`

**ORDER BY handling**: CLI places ORDER BY outside parentheses correctly.

**Skip**: If JQL/CQL already contains "project"/"space" keyword.

## Error Patterns

**Common errors**:
- `401 Unauthorized`: Check `ATLASSIAN_EMAIL` and `ATLASSIAN_API_TOKEN`
- `403 Forbidden`: Insufficient permissions for project/space
- `404 Not Found`: Invalid issue key or page ID
- `400 Bad Request`: JQL/CQL syntax error or invalid field names
- Network errors: Check domain, network connectivity

**Debugging**: Use `-v` flag for verbose logs (stderr).

## ID Discovery

**Comment ID**: From get issue response
```bash
comment_id=$(atlassian jira get PROJ-123 | jq -r '.fields.comment.comments[0].id')
atlassian jira comment update PROJ-123 "$comment_id" "Updated text"
```

**Page ID**: From search results or URL
```bash
page_id=$(atlassian confluence search "title=MyPage" | jq -r '.items[0].id')
atlassian confluence get "$page_id"
```

**Transition ID**: From transitions list
```bash
trans_id=$(atlassian jira transitions PROJ-123 | jq -r '.[] | select(.name=="In Progress").id')
atlassian jira transition PROJ-123 "$trans_id"
```

## Shell Patterns

**Quote escaping in JQL/CQL**:
```bash
atlassian jira search "summary ~ \"bug fix\""
```

**JSON escaping**:
```bash
# Use single quotes (no variable expansion)
atlassian jira update PROJ-123 '{"summary":"New title"}'

# With variables: double quotes + escape
title="Bug fix"
atlassian jira update PROJ-123 "{\"summary\":\"$title\"}"
```

**Multi-line content**:
```bash
# From file
atlassian confluence create SPACE "Title" "$(cat page.html)"

# Heredoc
content="$(cat <<'EOF'
Line 1
Line 2 with "quotes"
EOF
)"
atlassian jira create PROJ "Title" Bug --description "$content"
```

**jq filtering**:
```bash
# Extract keys
atlassian jira search "status=Open" | jq -r '.items[].key'

# Filter by field
atlassian jira search "project=PROJ" | jq -r '.items[] | select(.status.name=="Open") | .key'
```

**Bulk operations**:
```bash
# xargs (serial)
atlassian jira search "status=Open" | jq -r '.items[].key' | \
  xargs -I {} atlassian jira comment add {} "Bulk comment"

# for loop (with error handling)
for key in $(atlassian jira search "..." | jq -r '.items[].key'); do
  atlassian jira transition "$key" 31 || echo "Failed: $key"
done

# Parallel execution (4 concurrent)
... | xargs -P 4 -I {} atlassian jira comment add {} "Comment"
```

**Exit codes**:
```bash
# 0 = success, non-zero = error
set -e  # Exit on error

# Conditional execution
if result=$(atlassian jira get PROJ-123 2>/dev/null); then
  echo "Success: $result"
else
  echo "Failed with exit code: $?"
fi
```

## Response Structure (for jq parsing)

**Search responses**:
```json
{
  "items": [...],    // Array of issues/pages
  "total": N         // Total count (NOT "size"/"totalSize")
}
```

**Single item** (get/create):
```json
{
  "key": "PROJ-123",  // Jira issue key
  "id": "12345",      // Numeric ID
  "fields": {...}     // Issue/page fields
}
```

**Empty success** (update/transition): `{}`

## Pagination & Limits

**No pagination**: CLI uses `--limit` only (no startAt parameter).

**Defaults**: Jira search: 20, Confluence search: 10

**Workaround**: Increase `--limit` (e.g., `--limit 100`) or use JQL/CQL date ranges to chunk large datasets.

## Config Quick Reference

**TOML structure**:
```toml
[default]
domain = "company.atlassian.net"
email = "user@example.com"

[default.jira]
projects_filter = ["PROJ1"]
search_default_fields = ["key", "summary", "status"]
search_custom_fields = ["customfield_10015"]

[default.confluence]
spaces_filter = ["SPACE1"]
custom_includes = ["ancestors", "history"]

[work]  # Multi-tenant profile
domain = "work.atlassian.net"
email = "me@work.com"
```

**Key environment variables**:
- `JIRA_SEARCH_DEFAULT_FIELDS`: Override 17 defaults
- `JIRA_SEARCH_CUSTOM_FIELDS`: Extend defaults
- `CONFLUENCE_CUSTOM_INCLUDES`: ancestors, children, history, operations, labels, properties

## Query Patterns (CLI-specific)

**ORDER BY with auto-injection**: CLI handles correctly
```bash
"status != Done ORDER BY priority DESC"
# Becomes: "project IN (PROJ) AND (status != Done) ORDER BY priority DESC"
```

**JQL/CQL functions**: `assignee = currentUser()`, `created >= -7d`, `startOfDay()` (standard, pre-trained knowledge)

## Output & Debugging

**Output streams**:
- **stdout**: Compact JSON (default). Use `--pretty` for formatted output.
- **stderr**: Logs and errors. Suppress with `2>/dev/null`.

**jq pipeline**:
```bash
atlassian jira search "assignee=currentUser()" | jq -r '.items[].key'
```

## Performance Tuning (Optional)

**Environment variables**:
```bash
REQUEST_TIMEOUT_MS=60000   # Increase for slow networks (default: 30000)
MAX_CONNECTIONS=200        # Increase for bulk operations (default: 100)
```

## Config Commands (5 utilities)

```bash
atlassian config init [--global]     # Create config file
atlassian config show                # Display current config (token masked)
atlassian config list                # List config locations + env vars
atlassian config path [--global]     # Print config file path
atlassian config edit [--global]     # Open config in $EDITOR
```

## Testing This Skill

**Activation test**: Ask "Show me Jira issues assigned to me"

**Expected**: Claude uses `atlassian jira search "assignee = currentUser()"`

**Verification**: Ask "Create a Confluence page about X"

**Expected**: Claude uses `atlassian confluence create <space> "<title>" "<html content>"`
