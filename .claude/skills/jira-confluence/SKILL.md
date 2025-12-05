---
name: jira-confluence
description: Execute Jira/Confluence queries via atlassian CLI. Search issues with JQL, manage pages with CQL, create/update tickets, handle comments and transitions, work with ADF format. Supports full pagination with --all flag. Use when working with Jira tickets, Confluence pages, sprint planning, issue tracking, or Atlassian workspace queries.
allowed-tools: Bash
---

# atlassian-cli: Jira & Confluence CLI

## Quick Start

```bash
atlassian-cli --version
atlassian-cli config show
```

## Authentication (4-Tier Priority)

1. **CLI flags**: `--domain company.atlassian.net --email user@example.com --token TOKEN`
2. **Environment**: `ATLASSIAN_DOMAIN`, `ATLASSIAN_EMAIL`, `ATLASSIAN_API_TOKEN`
3. **Project config**: `./.atlassian.toml`
4. **Global config**: `~/.config/atlassian-cli/config.toml`

Domain format: `company.atlassian.net` (NOT `https://company.atlassian.net`)

## Jira Operations

### Search Issues (JQL)
```bash
atlassian-cli jira search "assignee = currentUser() AND status != Done" --limit 50
atlassian-cli jira search "project = PROJ ORDER BY priority DESC" --fields key,summary,status
```

**Field optimization** (60-70% reduction):
- Default 17 fields exclude `description`, `id`, `renderedFields`
- Override: `--fields key,summary,status`
- Env: `JIRA_SEARCH_DEFAULT_FIELDS`, `JIRA_SEARCH_CUSTOM_FIELDS`

### Get/Create/Update Issue
```bash
atlassian-cli jira get PROJ-123
atlassian-cli jira create PROJ "Bug title" Bug --description "Plain text"
atlassian-cli jira update PROJ-123 '{"summary": "Updated title"}'
```

### Comments & Transitions
```bash
atlassian-cli jira comment add PROJ-123 "Comment text"
atlassian-cli jira transitions PROJ-123
atlassian-cli jira transition PROJ-123 31
```

## Confluence Operations

### Search Pages (CQL)
```bash
# Basic search
atlassian-cli confluence search "title ~ 'Meeting Notes'" --limit 20

# Full pagination (all results)
atlassian-cli confluence search "space = TEAM" --all

# JSONL streaming (memory efficient for large results)
atlassian-cli confluence search "space = TEAM" --all --stream

# With expanded fields
atlassian-cli confluence search "type=page" --expand body.storage,ancestors
```

**Pagination options**:
- `--limit N`: Max results per request (default: 10, max: 250)
- `--all`: Fetch all results via cursor-based pagination
- `--stream`: Output JSONL format (requires --all)
- `--expand`: Expand fields (body.storage, ancestors, version, etc.)

### Get/Create/Update Page
```bash
atlassian-cli confluence get 12345
atlassian-cli confluence create SPACE "Page Title" "<p>HTML content</p>"
atlassian-cli confluence update 12345 "Updated Title" "<p>New content</p>"
```

Use space KEY (e.g., "TEAM"), not ID. Content: HTML storage format (NOT Markdown).

### Children & Comments
```bash
atlassian-cli confluence children 12345
atlassian-cli confluence comments 12345
```

## Config Commands

```bash
atlassian-cli config init [--global]
atlassian-cli config show
atlassian-cli config path [--global]
atlassian-cli config edit [--global]
```

## Advanced Patterns

### Bulk Operations
```bash
# Serial with error handling
for key in $(atlassian-cli jira search "status=Open" | jq -r '.items[].key'); do
  atlassian-cli jira comment add "$key" "Comment" || echo "Failed: $key"
done

# Parallel (4 concurrent)
atlassian-cli jira search "status=Open" | jq -r '.items[].key' | \
  xargs -P 4 -I {} atlassian-cli jira comment add {} "Comment"
```

### Project/Space Auto-Injection
```toml
# .atlassian.toml
[default.jira]
projects_filter = ["PROJ1", "PROJ2"]

[default.confluence]
spaces_filter = ["SPACE1"]
```

Effect: JQL becomes `project IN (PROJ1,PROJ2) AND (your_jql)`

### Multi-line Content
```bash
atlassian-cli confluence create SPACE "Title" "$(cat page.html)"
atlassian-cli jira create PROJ "Title" Bug --description "$(cat <<'EOF'
Line 1
Line 2
EOF
)"
```

## Common Workflows

```bash
# Daily standup: my issues updated today
atlassian-cli jira search "assignee = currentUser() AND updated >= startOfDay()" \
  --fields key,summary,status | jq -r '.items[] | "\(.key): \(.fields.summary)"'

# Export all Confluence pages in a space
atlassian-cli confluence search "space=TEAM AND type=page" --all --stream > pages.jsonl

# Meeting notes with date
atlassian-cli confluence create TEAM "Notes $(date +%Y-%m-%d)" "<h1>Attendees</h1><ul><li>Person 1</li></ul>"
```
