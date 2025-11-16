---
name: jira-confluence
version: 0.1.0
description: Execute Jira/Confluence queries via atlassian CLI. Search issues with JQL, manage pages with CQL, create/update tickets, handle comments and transitions, work with ADF format. Use when working with Jira tickets, Confluence pages, sprint planning, issue tracking, or Atlassian workspace queries.
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

## Jira Operations (7 Commands)

### Get Issue
```bash
atlassian-cli jira get PROJ-123
```

### Search Issues (JQL)
```bash
atlassian-cli jira search "assignee = currentUser() AND status != Done" --limit 50
atlassian-cli jira search "project = PROJ ORDER BY priority DESC" --fields key,summary,status
```

**Field optimization** (60-70% reduction):
- Default 17 fields exclude `description`, `id`, `renderedFields`
- Override: `--fields key,summary,status` (highest priority)
- Environment: `JIRA_SEARCH_DEFAULT_FIELDS=key,summary` (replaces defaults)
- Environment: `JIRA_SEARCH_CUSTOM_FIELDS=customfield_10015` (extends defaults)

### Create Issue
```bash
atlassian-cli jira create PROJ "Bug title" Bug --description "Plain text description"
```

### Update Issue
```bash
atlassian-cli jira update PROJ-123 '{"summary": "Updated title"}'
atlassian-cli jira update PROJ-123 '{"description": "New description"}'
```

### Comments
```bash
# Add comment
atlassian-cli jira comment add PROJ-123 "Comment text"

# Update comment
comment_id=$(atlassian-cli jira get PROJ-123 | jq -r '.fields.comment.comments[0].id')
atlassian-cli jira comment update PROJ-123 "$comment_id" "Updated text"
```

### Transitions
```bash
# List available transitions
atlassian-cli jira transitions PROJ-123

# Execute transition
trans_id=$(atlassian-cli jira transitions PROJ-123 | jq -r '.[] | select(.name=="In Progress").id')
atlassian-cli jira transition PROJ-123 "$trans_id"
```

## Confluence Operations (6 Commands)

### Search Pages (CQL)
```bash
atlassian-cli confluence search "title ~ 'Meeting Notes'" --limit 20
atlassian-cli confluence search "space = TEAM AND created >= now()-7d"
```

### Get Page
```bash
atlassian-cli confluence get 12345
```

### Page Children & Comments
```bash
atlassian-cli confluence children 12345
atlassian-cli confluence comments 12345
```

### Create Page
```bash
atlassian-cli confluence create SPACE "Page Title" "<p>HTML content with <strong>formatting</strong></p>"
```

Use space KEY (e.g., "TEAM"), not ID. Content format: HTML storage format (NOT Markdown).

### Update Page
```bash
atlassian-cli confluence update 12345 "Updated Title" "<p>New content</p>"
```

Version handling: CLI auto-increments version (no manual version needed).

## Advanced Patterns

### Bulk Operations
```bash
# Serial execution with error handling
for key in $(atlassian-cli jira search "status=Open" --limit 100 | jq -r '.items[].key'); do
  atlassian-cli jira comment add "$key" "Bulk comment" || echo "Failed: $key"
done

# Parallel execution (4 concurrent)
atlassian-cli jira search "status=Open" | jq -r '.items[].key' | \
  xargs -P 4 -I {} atlassian-cli jira comment add {} "Comment"
```

### Project/Space Auto-Injection
```toml
# .atlassian.toml or ~/.config/atlassian-cli/config.toml
[default.jira]
projects_filter = ["PROJ1", "PROJ2"]

[default.confluence]
spaces_filter = ["SPACE1"]
```

Effect: JQL becomes `project IN (PROJ1,PROJ2) AND (your_jql)`
ORDER BY is correctly placed outside parentheses.

### Multi-line Content
```bash
# From file
atlassian-cli confluence create SPACE "Title" "$(cat page.html)"

# Heredoc
content="$(cat <<'EOF'
Line 1
Line 2 with "quotes"
EOF
)"
atlassian-cli jira create PROJ "Title" Bug --description "$content"
```

### JSON Escaping
```bash
# Single quotes (no variable expansion)
atlassian-cli jira update PROJ-123 '{"summary":"New title"}'

# With variables: double quotes + escape
title="Bug fix"
atlassian-cli jira update PROJ-123 "{\"summary\":\"$title\"}"
```

### JQL/CQL Quote Escaping
```bash
atlassian-cli jira search "summary ~ \"bug fix\""
```

## Config Commands (5 Commands)

```bash
atlassian-cli config init [--global]     # Create config file
atlassian-cli config show                # Display current config (token masked)
atlassian-cli config list                # List config locations + env vars
atlassian-cli config path [--global]     # Print config file path
atlassian-cli config edit [--global]     # Open config in $EDITOR
```

## Common Workflows

```bash
# Daily standup: my issues updated today
atlassian-cli jira search "assignee = currentUser() AND updated >= startOfDay()" \
  --fields key,summary,status | jq -r '.items[] | "\(.key): \(.fields.summary)"'

# Bulk transition: move bugs to In Progress
trans_id=$(atlassian-cli jira transitions PROJ-1 | jq -r '.[] | select(.name=="In Progress").id')
atlassian-cli jira search "status=Open AND issuetype=Bug" | jq -r '.items[].key' | \
  xargs -I {} atlassian-cli jira transition {} "$trans_id"

# Meeting notes with date
atlassian-cli confluence create TEAM "Notes $(date +%Y-%m-%d)" "<h1>Attendees</h1><ul><li>Person 1</li></ul>"
```
