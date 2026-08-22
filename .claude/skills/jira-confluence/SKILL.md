---
name: jira-confluence
version: 0.12.0
description: Run Jira/Confluence operations through atlassian-cli — JQL/CQL search, issue CRUD, comments, transitions, issue links, worklogs, watchers, sprint/board/epic moves; Confluence page CRUD, comment threads (footer and inline, replies included), labels, content properties, spaces, and attachment upload, with ADF/HTML body editing. Also handles OAuth sign-in flows (`auth login/status/refresh`) when the user reports an auth problem or asks to switch accounts, and the tool's own installation (`self status/update/skill install`).
when_to_use: Trigger on Jira tickets, Confluence pages, sprint planning, time logging, "내 이슈", "위키 검색", auth trouble or account switching, updating atlassian-cli itself, or any Atlassian workspace request.
allowed-tools: Bash
---

# atlassian-cli

Always pass **global flags before the subcommand**:

```bash
atlassian-cli --profile work jira get PROJ-123 --format markdown
atlassian-cli --pretty confluence search "space = TEAM" --limit 5
```

`--format` defaults to `html`; pass `--format markdown` for human-readable output (ADF→Markdown for Jira, HTML→Markdown for Confluence). Either way the JSON envelope is preserved — the flag only converts the content fields (description, body) in place, it is not pure-markdown output. Pick markdown when you'll summarise, the JSON default when piping.

## Jira

```bash
# Read
atlassian-cli jira get PROJ-123 --format markdown
atlassian-cli jira get PROJ-123 --fields '*all'             # full issue incl. custom fields
atlassian-cli jira get PROJ-123 --fields summary,status,customfield_10020
atlassian-cli jira search "assignee = currentUser() AND status != Done" --format markdown --limit 20
atlassian-cli jira search "project = PROJ" --fields key,summary,status --limit 50
atlassian-cli jira comment list PROJ-123 --format markdown
atlassian-cli jira transition list PROJ-123      # discover IDs before applying

# Large reads — token pagination
atlassian-cli jira search "project = PROJ" --all --format markdown
atlassian-cli jira search "project = PROJ" --all --stream > issues.jsonl

# Write — plain text auto-converts to ADF
atlassian-cli jira create PROJ "Summary" Bug --description "Plain text"
atlassian-cli jira create PROJ "Summary" Sub-task --parent PROJ-123   # every sub-task type needs a parent
atlassian-cli jira create PROJ "Summary" Task --fields '{"components":[{"name":"api"}]}'   # anything else the create screen requires
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
atlassian-cli jira link remove PROJ-1 PROJ-2 --type Blocks   # by issue pair; source is the OUTWARD side, as in `add`
atlassian-cli jira link remove --id 10001                    # by the link's own id, as `link list` reports it —
                                                             # the way through any refusal the pair form cannot settle
                                                             # (two identical links, an entry it could not read)

# Worklogs — time format is "2h 30m" / "1d" / "45m"
atlassian-cli jira worklog add PROJ-123 "2h 30m" --comment "Investigation"
atlassian-cli jira worklog add PROJ-123 "2h" --started 2026-08-22T09:30:00.000+0900   # backdate; omitted logs now
atlassian-cli jira worklog list PROJ-123
atlassian-cli jira worklog update PROJ-123 10001 "3h"   # worklog id from `worklog list`
atlassian-cli jira worklog remove PROJ-123 10001

# Watchers — operate on the signed-in user
atlassian-cli jira watcher add PROJ-123
atlassian-cli jira watcher list PROJ-123
atlassian-cli jira watcher remove PROJ-123
```

### Discovery (global metadata)

`list types` answers site-wide, not per project: for an ordinary user it is the union over projects they can browse, and for a Jira admin it is every type on the site including ones no project uses. Either way a name it returns may be refused by `create` in the project you are creating in. `jira get KEY --fields issuetype` on an issue already in that project gives a name that project uses — which is a better starting point than this list, though it is not proof the type can still be created there.

```bash
atlassian-cli jira list types        # site-wide; see the note above before using one with `create`
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

`epic assign`/`unassign` drive the agile **Epic Link** endpoint, which only takes effect on **company-managed** projects. Team-managed (next-gen) projects model the epic as the issue's `parent`, so there the command returns success but has no effect — pass `--parent EPIC-1` when creating, or set it after with `jira update KEY '{"parent":{"key":"EPIC-1"}}'` (when the project's field config allows it). A sub-task is the same field and cannot be created without it: Jira takes the parent on the create screen, so there is no issue to update into place.

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

A string arg is treated as ADF **only** if it parses to a complete valid ADF document; any other string — plain prose, or JSON-shaped text like `{"status":"done"}` — is posted verbatim as the literal body. So passing partial/invalid ADF silently lands as text rather than erroring; send a full, valid `doc` when you mean rich text.

## Confluence

```bash
# Read
atlassian-cli confluence get 12345 --format markdown
atlassian-cli confluence search "space = TEAM" --limit 20
atlassian-cli confluence children 12345          # children is JSON only (no --format)
atlassian-cli confluence comment list 12345 --format markdown   # every reply, footer and inline

# Large reads — cursor pagination (list endpoints auto-follow cursors to completion)
atlassian-cli confluence search "space = TEAM" --all --stream > pages.jsonl

# Pages — HTML storage format required
atlassian-cli confluence create SPACE "Title" "<p>Content</p>"
atlassian-cli confluence create SPACE "Title" "<p>Content</p>" --parent 12345  # nest under a page
atlassian-cli confluence update 12345 "Title" "<p>Updated</p>"
atlassian-cli confluence update 12345 "Title" "<p>Updated</p>" --parent 67890  # re-parent
atlassian-cli confluence delete 12345 --yes      # moves to trash (recoverable)

# Comments — reads cover both families and every reply; writes are footer-only
atlassian-cli confluence comment list 12345 --location inline   # just the anchored ones
atlassian-cli confluence comment list 12345 --roots-only        # top level only
atlassian-cli confluence comment get 67890                      # one comment by id
atlassian-cli confluence comment replies 67890                  # one thread
atlassian-cli confluence comment get 67890 --location inline
atlassian-cli confluence comment add 12345 "<p>Looks good</p>"
atlassian-cli confluence comment add 12345 "<p>Reply</p>" --reply-to 67890
atlassian-cli confluence comment update 67890 "<p>Edited</p>"   # by comment id, not page id
atlassian-cli confluence comment delete 67890

# Labels
atlassian-cli confluence label add 12345 needs-review
atlassian-cli confluence label list 12345
atlassian-cli confluence label remove 12345 needs-review

# Content properties — structured JSON metadata on a page (machine-read state store)
atlassian-cli confluence property set 12345 review '{"status":"done"}'
atlassian-cli confluence property list 12345
atlassian-cli confluence property delete 12345 review

# Spaces & attachments
atlassian-cli confluence space list
atlassian-cli confluence space get TEAM
atlassian-cli confluence attachment list 12345
atlassian-cli confluence attachment upload 12345 ./diagram.png --comment "v2"
atlassian-cli confluence attachment upload 12345 ./icon.svg --content-type image/svg+xml
```

- `comment list` returns one flat, depth-first array covering both comment families and every level of every thread. Each entry carries `location` (`footer`/`inline`), `depth`, and `parentCommentId` (`null` at a root), so rebuild the tree from those rather than assuming the array is top-level only.
- `comment get`/`replies`/`update`/`delete` take the **comment id**; `comment list`/`add` take the **page id**.
- Inline entries carry `resolutionStatus` (`open`/`reopened`/`resolved`/`dangling`) and `properties.inlineOriginalSelection`, the text they anchor to. Filter on `resolutionStatus` for "unresolved comments"; footer entries have neither field.
- `--location` defaults to `footer` on `comment get`/`replies` — an id from an inline thread needs `--location inline`, and every entry `comment list` returns already states which it is. A wrong `--location` is a 404, not a fallback.
- `property set` values are **strict JSON** — quote bare strings as `'"text"'`, not `text`.
- `attachment upload` upserts by filename; add `--minor` to suppress watcher notifications on re-upload. The `Content-Type` is mapped from the file extension (so `diagram.png` → `image/png` and renders inline instead of becoming an opaque download); pass `--content-type <mime>` to override. Note: Confluence Cloud often blocks **inline SVG** rendering for security, so embed diagrams as PNG when you need them to display.
- Under OAuth, Confluence v2 writes need granular scopes on the token, not just classic ones. A `401 "scope does not match"` means the scope was never requested at login: add it to the profile's `scopes` in config and re-run `auth login` (having it enabled on the OAuth app is not enough — the token only carries scopes the login *requested*). The write↔scope map: comment → `write:comment:confluence`, property → `write:content:confluence`, attachment → `write:attachment:confluence`, page create/update → `write:page:confluence`, page delete → `delete:page:confluence`, `space`/page-create space lookup → `read:space:confluence`.
- CQL: searching by user requires account IDs or public names — username fields are restricted in Atlassian Cloud.

## Pagination & output cheatsheet

| Need | Flag |
|---|---|
| One page | (default `--limit N`) |
| Every result | `--all` |
| Stream to disk / pipe | `--all --stream` (outputs JSONL) |
| Pick fields (Jira `get` & `search`) | `--fields key,summary,status` (or `*all` on `get` for the full issue) |
| Expand fields (Confluence `search` only) | `--expand ancestors,space` — `get` has no `--expand`; read `spaceId` from `get`, then map it via `space list` |

`--stream` writes JSONL to stdout and progress to stderr — never mix it with `--pretty`.

## Errors & exit codes

Failures print a single-line JSON object to **stderr**; stdout stays results-only:

```json
{"error":{"message":"Failed to get issue (404 Not Found): ...","operation":"get issue","status":404}}
```

Parse `status`/`operation` instead of regexing the message; `hint` (present only when a known remediation exists, e.g. a 401 from an API token sent to the host that refuses its shape) is the remediation to relay to the user. Exit codes: `0` ok, `1` generic, `2` usage, `3` auth (401/403), `4` not found, `5` rate limited (429 — already retried 3× with `Retry-After`; back off before rerunning), `6` server error (5xx).

## Authentication

Credentials are pre-configured. **Do not print, request, infer, or modify secrets.**

The active profile dictates what identity the call runs as:

| profile method | who calls Atlassian |
|---|---|
| `oauth` | the signed-in human (token in OS keychain, auto-refreshed) |
| `scoped_token` | the API-token owner, limited to the scopes on the token (API token **with** scopes) |
| `basic` | the API-token owner, with their full permissions (classic/**unscoped** token) |
| `service_account` | a non-human service principal |

`basic` and `scoped_token` differ only in which host the token goes to, and each host accepts exactly one token shape. A 401 on either carries a `hint` naming the other — relay it; the fix is a config change, not a retry.

Run `atlassian-cli config validate` first when a request will write or fetch many pages — it prints the resolved identity and fails fast on bad credentials. Caveat: the identity probe hits a Jira endpoint, so on any Confluence-only-scoped profile (`oauth` or `scoped_token`) it returns a 401 "scope does not match" even when the profile works — treat that specific failure as expected and confirm with a cheap read (e.g. `space list`) instead.

When the user reports auth trouble or asks to switch accounts:

```bash
atlassian-cli auth status                # expiry, scopes, storage backend
atlassian-cli auth login                 # OAuth 3LO; opens browser
atlassian-cli auth login --no-browser    # SSH session — prints the URL
atlassian-cli auth refresh               # force token refresh (debugging)
atlassian-cli auth logout                # clears the stored session, whatever method the profile is on
```

Switch profiles with `--profile <name>`; never invent profile names — list them via `atlassian-cli config list`.

## The installation itself

```bash
atlassian-cli self status                # version, binary path, skill state, profiles holding tokens — no network
atlassian-cli self update                # checks GitHub, replaces only after the download runs and reports the expected version
atlassian-cli self update --version 0.10.0  # a downgrade needs the version named; below 0.10.0 there is no `self` to come back with
atlassian-cli self skill install         # rewrite this skill from the running binary
```

The skill is compiled into the binary, so `self status` reporting `stale` means the deployed copy differs byte-for-byte from what this binary carries — `self skill install` is the fix, and the two can never be different versions. `self update` changes nothing when the binary is already current. `self uninstall` and `self skill remove` remove things and take `--yes`; leave them to the user rather than running them on your own initiative.

## Behaviour worth knowing

- A `projects_filter` or `spaces_filter` on the active profile is auto-injected into bare JQL/CQL. If the user already names a project/space in their query, no second filter is added — write the query the user said.
- `jira search` uses `POST /rest/api/3/search/jql` under the hood — token-paginated, not offset.
- `--format markdown` on reads keeps the JSON envelope and converts the content fields in place; it isn't pure markdown output.
