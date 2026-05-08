---
name: jira-confluence
version: 0.1.2
description: Execute Jira/Confluence queries via atlassian-cli. Search issues with JQL, manage pages with CQL, create/update tickets, handle comments and transitions, work with ADF format. Use when working with Jira tickets, Confluence pages, sprint planning, issue tracking, or Atlassian workspace queries.
allowed-tools: Bash
---

# atlassian-cli

Atlassian Cloud CLI for Jira and Confluence. Use `--format markdown` for human-readable content; JSON remains the canonical machine-readable output.

Global options must come before the service subcommand:

```bash
atlassian-cli --profile work --pretty jira get PROJ-123
atlassian-cli --config ./.atlassian.toml confluence search "space = TEAM"
atlassian-cli -vv jira search "project = PROJ"
```

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

# Pagination (large datasets)
atlassian-cli jira search "project = PROJ" --all --format markdown
atlassian-cli jira search "project = PROJ" --all --stream > issues.jsonl

# Create/Update
atlassian-cli jira create PROJ "Summary" Bug --description "Plain text"
atlassian-cli jira update PROJ-123 '{"summary": "New title", "description": "Plain text"}'

# Comments & Transitions
atlassian-cli jira comment add PROJ-123 "Comment text"
atlassian-cli jira comment update PROJ-123 10042 "Edited comment"
atlassian-cli jira comments PROJ-123 --format markdown
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

# Search with content (body included by default)
atlassian-cli confluence search "title ~ 'API'" --format markdown --limit 10

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
| `--format markdown` | Convert to Markdown | search, get, comments |
| `--limit N` | Max results (default: 10, max: 50 with body) | search |
| `--all` | Fetch all pages via cursor pagination | search |
| `--stream` | Output JSONL (requires --all) | search |
| `--expand <fields>` | Additional fields: `ancestors`, `space` (body.storage included by default) | search |

Note: `children` does not support `--format` (v2 API limitation).

## Common Options (Both Jira & Confluence)

| Option | Jira | Confluence |
|--------|------|------------|
| `--all` | Token pagination | Cursor pagination |
| `--stream` | JSONL output (requires --all) | JSONL output (requires --all) |
| `--format markdown` | ADF → Markdown | HTML → Markdown |
| `--limit N` | Results per page (default: 100) | Results per page (default: 10) |

## Authentication

Assume credentials are already configured. Do not print, request, or infer secrets.

Before destructive writes or large `--all` reads, validate the active profile:

```bash
atlassian-cli config validate
```

If auth fails, tell the user to check `atlassian-cli config validate` output. Only use `--profile` or `--config` when the user specifies which Atlassian workspace/profile to target.

## Query Behavior

- Jira/Confluence project or space filters may be preconfigured and auto-injected.
- If a user explicitly names a project or space in the query, the CLI does not add a duplicate filter.
- Use `--fields` for Jira searches when the user only needs a few fields; use `--all --stream` for large machine-readable exports.

## API Version Notes

- Jira issue search uses `POST /rest/api/3/search/jql`.
- Confluence page, space, and comment APIs use `/wiki/api/v2/*`.
- Confluence CQL search intentionally uses `/wiki/rest/api/search`; v2 does not provide a CQL-equivalent search endpoint.
- Confluence CQL user-specific fields are restricted by Atlassian Cloud. Prefer account IDs/public names where applicable.
