---
name: jira-confluence
description: Execute Jira/Confluence queries via atlassian-cli. Search issues with JQL, manage pages with CQL, create/update tickets, handle comments and transitions, work with ADF format. Use when working with Jira tickets, Confluence pages, sprint planning, issue tracking, or Atlassian workspace queries.
allowed-tools: Bash
---

# atlassian-cli

## Jira

**Reading**: `--format markdown` converts ADF to Markdown (recommended)
**Writing**: Plain text auto-converts to ADF. For rich text, use ADF JSON.

### Commands
```bash
# Get issue
atlassian-cli jira get PROJ-123 --format markdown

# Search (JQL)
atlassian-cli jira search "assignee = currentUser()" --format markdown --limit 20
atlassian-cli jira search "project = PROJ" --fields key,summary,status --limit 50

# Create/Update
atlassian-cli jira create PROJ "Summary" Bug --description "Plain text"
atlassian-cli jira update PROJ-123 '{"summary": "New title", "description": "Plain text"}'

# Comments & Transitions
atlassian-cli jira comment add PROJ-123 "Comment text"
atlassian-cli jira transitions PROJ-123
atlassian-cli jira transition PROJ-123 31
```

### ADF Format (for rich text)

Root: `{"version": 1, "type": "doc", "content": [...]}`

| Node | Example |
|------|---------|
| paragraph | `{"type": "paragraph", "content": [{"type": "text", "text": "..."}]}` |
| heading | `{"type": "heading", "attrs": {"level": 2}, "content": [...]}` |
| bulletList | `{"type": "bulletList", "content": [{"type": "listItem", "content": [...]}]}` |
| codeBlock | `{"type": "codeBlock", "attrs": {"language": "python"}, "content": [...]}` |

Marks: `{"type": "text", "text": "bold", "marks": [{"type": "strong"}]}`
- `strong`, `em`, `code`, `strike`, `link` (with `attrs.href`)

List hierarchy: `bulletList` → `listItem` → `paragraph` → `text`

## Confluence

**Reading**: `--format markdown` converts HTML to Markdown (recommended)
**Writing**: HTML storage format required (e.g., `<p>text</p>`)

### Commands
```bash
# Get page
atlassian-cli confluence get 12345 --format markdown

# Search (CQL) - metadata only (fast)
atlassian-cli confluence search "space = TEAM" --limit 20

# Search with content (requires --expand)
atlassian-cli confluence search "title ~ 'API'" --expand body.storage --format markdown --limit 10

# Pagination
atlassian-cli confluence search "space = TEAM" --all --format markdown
atlassian-cli confluence search "space = TEAM" --all --stream > pages.jsonl

# Create/Update (HTML format)
atlassian-cli confluence create SPACE "Title" "<p>Content</p>"
atlassian-cli confluence update 12345 "Title" "<p>Updated</p>"

# Children & Comments
atlassian-cli confluence children 12345
atlassian-cli confluence comments 12345 --format markdown
```

### Options
| Option | Description | Applies To |
|--------|-------------|------------|
| `--expand body.storage` | Include page content | search |
| `--format markdown` | Convert to Markdown | search, get, comments |
| `--limit N` | Max results (default: 10, max: 250) | search |
| `--all` | Fetch all pages via cursor pagination | search |
| `--stream` | Output JSONL (requires --all) | search |

Note: `children` does not support `--format` (v2 API limitation).

## Authentication

Priority: CLI flags > Environment > Project config > Global config

```bash
# Environment variables
export ATLASSIAN_DOMAIN=company.atlassian.net
export ATLASSIAN_EMAIL=user@example.com
export ATLASSIAN_API_TOKEN=token
```

## Auto-Injection Filter

```toml
# .atlassian.toml
[default.jira]
projects_filter = ["PROJ1", "PROJ2"]

[default.confluence]
spaces_filter = ["SPACE1"]
```

Effect: JQL becomes `project IN (PROJ1,PROJ2) AND (your_jql)`
