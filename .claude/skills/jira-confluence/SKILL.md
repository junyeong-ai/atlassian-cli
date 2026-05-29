---
name: jira-confluence
version: 0.5.0
description: Run Jira/Confluence operations through atlassian-cli — JQL/CQL search, issue CRUD, comments, transitions, issue links, worklogs, watchers, sprint/board/epic moves, and Confluence page CRUD with ADF/HTML body editing. Also handles OAuth sign-in flows (`auth login/status/refresh`) when the user reports an auth problem or asks to switch accounts. Trigger on Jira tickets, Confluence pages, sprint planning, time logging, "내 이슈", "위키 검색", or any Atlassian workspace request.
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
atlassian-cli jira comment list PROJ-123 --format markdown
atlassian-cli jira transition list PROJ-123      # discover IDs before applying

# Large reads — token pagination
atlassian-cli jira search "project = PROJ" --all --format markdown
atlassian-cli jira search "project = PROJ" --all --stream > issues.jsonl

# Write — plain text auto-converts to ADF
atlassian-cli jira create PROJ "Summary" Bug --description "Plain text"
atlassian-cli jira update PROJ-123 '{"summary": "New title", "description": "Plain text"}'
atlassian-cli jira comment add PROJ-123 "Comment text"
atlassian-cli jira comment update PROJ-123 10042 "Edited comment"
atlassian-cli jira comment delete PROJ-123 10042
atlassian-cli jira transition apply PROJ-123 31

# Delete — irreversible (no recycle bin); --yes is mandatory
atlassian-cli jira delete PROJ-123 --yes
atlassian-cli jira delete PROJ-123 --yes --delete-subtasks
```

### Links, worklogs, watchers

```bash
# Issue links — `add` takes source then target; source is the OUTWARD side
# ("A blocks B" → source=A). Discover type names with `link types`.
atlassian-cli jira link types
atlassian-cli jira link add PROJ-1 PROJ-2 --type Blocks
atlassian-cli jira link list PROJ-1
atlassian-cli jira link remove PROJ-1 PROJ-2 --type Blocks   # by issue pair, not link ID

# Worklogs — time format is "2h 30m" / "1d" / "45m"
atlassian-cli jira worklog add PROJ-123 "2h 30m" --comment "Investigation"
atlassian-cli jira worklog list PROJ-123

# Watchers — operate on the signed-in user
atlassian-cli jira watcher add PROJ-123
atlassian-cli jira watcher list PROJ-123
```

### Discovery (global metadata)

```bash
atlassian-cli jira list types        # issue types — names for `create`
atlassian-cli jira list priorities   # priority names for `update`
atlassian-cli jira list statuses     # status names for JQL / transitions
atlassian-cli jira list labels       # existing labels
```

### Agile — boards, sprints, epics

```bash
atlassian-cli jira board list --project PROJ        # find the board id
atlassian-cli jira sprint list --project PROJ       # auto-resolves the board
atlassian-cli jira sprint list --board 42 --state active
atlassian-cli jira sprint move 55 PROJ-1 PROJ-2     # move issues into sprint 55
atlassian-cli jira sprint backlog PROJ-1            # move back to backlog
atlassian-cli jira epic assign EPIC-1 PROJ-1 PROJ-2 # attach issues to an epic
atlassian-cli jira epic unassign PROJ-1             # detach from its epic
```

Board/sprint commands use the agile API. Pass `--project` to let the CLI resolve the board; if the project has several boards it lists them and asks for `--board`.

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
atlassian-cli confluence delete 12345 --yes      # moves to trash (recoverable)
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
