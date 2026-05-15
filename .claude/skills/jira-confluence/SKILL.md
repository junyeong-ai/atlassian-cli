---
name: jira-confluence
version: 0.3.0
description: Run Jira/Confluence operations through atlassian-cli — JQL/CQL search, issue CRUD, comments, transitions, page CRUD, ADF/HTML body editing. Also handles OAuth sign-in flows (`auth login/status/refresh`) when the user reports an auth problem or asks to switch accounts. Trigger on Jira tickets, Confluence pages, sprint planning, "내 이슈", "위키 검색", or any Atlassian workspace request.
allowed-tools: Bash
---

# atlassian-cli

Always pass **global flags before the subcommand**:

```bash
atlassian-cli --profile work jira get PROJ-123 --format markdown
atlassian-cli --pretty confluence search "space = TEAM" --limit 5
```

`--format markdown` is the default for human-readable output (ADF→Markdown for Jira, HTML→Markdown for Confluence). JSON is the canonical machine output — pick markdown when you'll summarise, JSON when piping.

## Jira

```bash
# Read
atlassian-cli jira get PROJ-123 --format markdown
atlassian-cli jira search "assignee = currentUser() AND status != Done" --format markdown --limit 20
atlassian-cli jira search "project = PROJ" --fields key,summary,status --limit 50
atlassian-cli jira comments PROJ-123 --format markdown
atlassian-cli jira transitions PROJ-123          # discover IDs before transition

# Large reads — token pagination
atlassian-cli jira search "project = PROJ" --all --format markdown
atlassian-cli jira search "project = PROJ" --all --stream > issues.jsonl

# Write — plain text auto-converts to ADF
atlassian-cli jira create PROJ "Summary" Bug --description "Plain text"
atlassian-cli jira update PROJ-123 '{"summary": "New title", "description": "Plain text"}'
atlassian-cli jira comment add PROJ-123 "Comment text"
atlassian-cli jira comment update PROJ-123 10042 "Edited comment"
atlassian-cli jira transition PROJ-123 31
```

### ADF (rich text — only when plain text isn't enough)

Root: `{"version": 1, "type": "doc", "content": [...]}`

| Node | Shape |
|---|---|
| paragraph | `{"type":"paragraph","content":[{"type":"text","text":"..."}]}` |
| heading | `{"type":"heading","attrs":{"level":2},"content":[...]}` |
| bulletList | `{"type":"bulletList","content":[{"type":"listItem","content":[<paragraph>]}]}` |
| codeBlock | `{"type":"codeBlock","attrs":{"language":"python"},"content":[<text>]}` |

Marks on text: `{"type":"text","text":"bold","marks":[{"type":"strong"}]}` — supports `strong`, `em`, `code`, `strike`, and `link` (`attrs.href`).

List nesting is strict: `bulletList → listItem → paragraph → text`.

## Confluence

```bash
# Read
atlassian-cli confluence get 12345 --format markdown
atlassian-cli confluence search "space = TEAM" --limit 20
atlassian-cli confluence search "title ~ 'API'" --format markdown --limit 10
atlassian-cli confluence comments 12345 --format markdown
atlassian-cli confluence children 12345          # children is JSON only (no --format)

# Large reads — cursor pagination
atlassian-cli confluence search "space = TEAM" --all --format markdown
atlassian-cli confluence search "space = TEAM" --all --stream > pages.jsonl

# Write — HTML storage format required
atlassian-cli confluence create SPACE "Title" "<p>Content</p>"
atlassian-cli confluence update 12345 "Title" "<p>Updated</p>"
```

CQL: searching by user requires account IDs or public names — username fields are restricted in Atlassian Cloud.

## Pagination & output cheatsheet

| Need | Flag |
|---|---|
| One page | (default `--limit N`) |
| Every result | `--all` |
| Stream to disk / pipe | `--all --stream` (outputs JSONL) |
| Pick fields (Jira) | `--fields key,summary,status` |
| Expand fields (Confluence) | `--expand ancestors,space` |

`--stream` writes JSONL to stdout and progress to stderr — never mix it with `--pretty`.

## Authentication

Credentials are pre-configured. **Do not print, request, infer, or modify secrets.**

The active profile dictates what identity the call runs as:

| profile method | who calls Atlassian |
|---|---|
| `oauth` | the signed-in human (token in OS keychain, auto-refreshed) |
| `basic` | the API-token owner |
| `service_account` | a non-human service principal |

Run `atlassian-cli config validate` first when a request will write or fetch many pages — it prints the resolved identity and fails fast on bad credentials.

When the user reports auth trouble or asks to switch accounts:

```bash
atlassian-cli auth status                # expiry, scopes, storage backend
atlassian-cli auth login                 # OAuth 3LO; opens browser
atlassian-cli auth login --no-browser    # SSH session — prints the URL
atlassian-cli auth refresh               # force token refresh (debugging)
atlassian-cli auth logout                # clears OAuth tokens; no-op on non-oauth profiles
```

Switch profiles with `--profile <name>`; never invent profile names — list them via `atlassian-cli config list`.

## Behaviour worth knowing

- A `projects_filter` or `spaces_filter` on the active profile is auto-injected into bare JQL/CQL. If the user already names a project/space in their query, no second filter is added — write the query the user said.
- `jira search` uses `POST /rest/api/3/search/jql` under the hood — token-paginated, not offset.
- `--format markdown` on reads keeps the JSON envelope and converts the content fields in place; it isn't pure markdown output.
