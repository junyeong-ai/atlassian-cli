---
name: jira-confluence
version: 0.14.0
description: Searches, reads and edits Atlassian Cloud Jira issues and Confluence pages through atlassian-cli, in Markdown both ways, including comments, links, worklogs, sprints, epics, attachments and user lookup. Use when the user mentions Jira, Confluence, an issue key, JQL or CQL, or a wiki page, or asks to diagnose atlassian-cli authentication or manage its installation.
allowed-tools: Bash
---

# atlassian-cli

Pass **root options before the subcommand**. Use `atlassian-cli --help` or `atlassian-cli <command> --help` for the installed version's exact arguments and defaults:

```bash
atlassian-cli --profile work jira get PROJ-123 --format markdown
atlassian-cli --pretty confluence search "space = TEAM" --limit 5
```

`--format` names how rich text is read and written. The default is each product's stored representation: `adf` (ADF JSON) for Jira and `storage` (XHTML) for Confluence. `--format markdown` converts in both directions. On reads, only the content fields inside the JSON envelope change; on writes, the body you pass is Markdown. Prefer Markdown for both, and use the stored representation only for content Markdown cannot express, such as panels, mentions or Confluence macros.

Write bodies through standard input with `-` whenever they span lines or contain quotes. A quoted heredoc needs no escaping and avoids the operating system's argument-size limit:

```bash
atlassian-cli jira comment add PROJ-123 - --format markdown <<'EOF'
Fixed in `parser.rs`; see **Release notes**.
EOF
```

`-` with nothing piped in is refused rather than posted as an empty body. Markdown follows CommonMark with GitHub tables and strikethrough: a single newline joins lines, so separate paragraphs with a blank line. Task-list checkboxes stay literal text.

## Jira

```bash
# Read
atlassian-cli jira get PROJ-123 --format markdown
atlassian-cli jira get PROJ-123 --fields '*all'             # full issue incl. custom fields
atlassian-cli jira get PROJ-123 --fields summary,status,customfield_10020
atlassian-cli jira search "assignee = currentUser() AND statusCategory != Done" --format markdown --limit 20
atlassian-cli jira search "project = PROJ" --fields key,summary,status --limit 50
atlassian-cli jira comment list PROJ-123 --format markdown
atlassian-cli jira transition list PROJ-123      # discover IDs before applying

# Large reads — token pagination
atlassian-cli jira search "project = PROJ" --all --format markdown
atlassian-cli jira search "project = PROJ" --all --stream > issues.jsonl

# Write — `--format markdown` converts the description or comment to ADF
atlassian-cli jira create PROJ "Summary" Bug --format markdown --description - < report.md
atlassian-cli jira create PROJ "Summary" Sub-task --parent PROJ-123   # every sub-task type needs a parent
atlassian-cli jira create PROJ "Summary" Task --fields '{"components":[{"name":"api"}]}'   # anything else the create screen requires
atlassian-cli jira update PROJ-123 '{"summary": "New title", "description": "## Cause\n\nA *race*"}' --format markdown
atlassian-cli jira update PROJ-123 '{"assignee": {"accountId": "5b10a2844c20165700ede21g"}}'
atlassian-cli jira comment add PROJ-123 "Reproduced on **staging**" --format markdown
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
atlassian-cli jira user search "Kim" --limit 5   # accountId for assignee fields, JQL and CQL
```

An `accountId` identifies the same person in Jira and Confluence. Assignee, reporter and user fields take `{"accountId": "..."}`; JQL and CQL compare against the bare id. User search matches the start of a display name or email, and an account without the *Browse users and groups* permission gets an empty list rather than an error. Under OAuth it needs `read:jira-user`.

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

An issue's epic is its `parent`, in company-managed and team-managed projects alike. `epic assign` sets it and `epic unassign` clears it, one issue at a time; a failure names the issue it stopped at and how many were already edited. Pass `--parent EPIC-1` to set it while creating. A sub-task's parent is the same field, required at creation and never cleared.

OAuth agile commands require endpoint-specific grants beyond core Jira scopes. Board discovery needs `read:board-scope:jira-software` and `read:project:jira`; sprint reads need `read:sprint:jira-software`; sprint moves need `write:sprint:jira-software`; moving to backlog needs `write:board-scope:jira-software`. Epic membership is an issue edit: it needs `write:jira-work`, and `epic unassign` also reads the issue back with `read:jira-work`. `sprint list --project` also performs board discovery. Use only the grants needed for the requested operations; see the [Software scope reference](https://developer.atlassian.com/cloud/jira/software/scopes-for-oauth-2-3LO-and-forge-apps/).

### Rich text

Markdown becomes ADF headings, lists, quotes, code blocks, tables, rules, links and the strong, emphasis, strike and code marks. ADF lets a list item or quote hold only paragraphs, lists and code blocks, so Markdown nesting a heading, quote, table or rule inside one is refused with its location; move it out. Images become their alt text linked to the image. Raw HTML stays literal text in both products.

Under the default `--format adf`, a string is used as ADF when it parses to an object with `type: "doc"`, `version: 1` and an array `content`. Other strings, including JSON-shaped text like `{"status":"done"}`, are posted as literal text. The CLI checks only this root structure; Jira validates the nested nodes and can reject a document that passes the local check.

An empty string becomes an empty ADF paragraph. Use `jira update KEY '{"description":null}'` to clear a description. Omitting `description` leaves it unchanged.

A depth-limit marker in Markdown means content was truncated; inspect the unconverted document with `--format adf`.

## Confluence

```bash
# Read
atlassian-cli confluence get 12345 --format markdown
atlassian-cli confluence search "space = TEAM" --limit 20
atlassian-cli confluence children 12345          # direct children of any type, each with its `type`; JSON only
atlassian-cli confluence comment list 12345 --format markdown   # every reply, footer and inline

# Large reads — cursor pagination (list endpoints auto-follow cursors to completion)
atlassian-cli confluence search "space = TEAM" --all --stream > pages.jsonl

# Pages — storage format by default, Markdown with --format markdown
atlassian-cli confluence create SPACE "Title" - --format markdown < page.md
atlassian-cli confluence create SPACE "Title" "<p>Content</p>" --parent 12345  # nest under a page
atlassian-cli confluence update 12345 "Title" - --format markdown --base-version 7 < page.md
atlassian-cli confluence update 12345 "Title" "<p>Updated</p>" --parent 67890  # re-parent
atlassian-cli confluence delete 12345 --yes      # moves to trash (recoverable)

# Comments — reads cover both families and every reply; writes are footer-only
atlassian-cli confluence comment list 12345 --location inline   # just the anchored ones
atlassian-cli confluence comment list 12345 --roots-only        # top level only
atlassian-cli confluence comment get 67890                      # one comment by id
atlassian-cli confluence comment replies 67890                  # one thread
atlassian-cli confluence comment get 67890 --location inline
atlassian-cli confluence comment add 12345 "Looks **good**" --format markdown
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

- To edit a page, read it, change the body, and write it back with `--base-version` set to the `version.number` you read. Without it the update overwrites whatever is current, including a save made while you worked; with it Confluence answers `409` when the page changed, and the fix is to re-read, reapply the edit and retry. A space that requires approval for edits can also answer `409`; its message says so.
- Markdown converts to storage format with code blocks as the `code` macro. An image whose source has a scheme (`https://…`) embeds that URL; any other source names the page attachment by its last path segment, the name `attachment upload` gives the file, so upload it first. Write Confluence macros and HTML with `--format storage`; in Markdown they stay literal text.
- `comment list` returns one flat, depth-first array covering both comment families and every level of every thread. Each entry carries `location` (`footer`/`inline`), `depth`, and `parentCommentId` (`null` at a root), so rebuild the tree from those rather than assuming the array is top-level only.
- `comment get`/`replies`/`update`/`delete` take the **comment id**; `comment list`/`add` take the **page id**.
- Inline entries carry `resolutionStatus` (`open`/`reopened`/`resolved`/`dangling`) and `properties.inlineOriginalSelection`, the text they anchor to. Filter on `resolutionStatus` for "unresolved comments"; footer entries have neither field.
- `--location` defaults to `footer` on `comment get`/`replies` — an id from an inline thread needs `--location inline`, and every entry `comment list` returns already states which it is. A wrong `--location` is a 404, not a fallback.
- `property set` values are **strict JSON** — quote bare strings as `'"text"'`, not `text`.
- `attachment upload` upserts by filename; add `--minor` to suppress watcher notifications on re-upload. The `Content-Type` is mapped from the file extension (so `diagram.png` → `image/png` and renders inline instead of becoming an opaque download); pass `--content-type <mime>` to override. Note: Confluence Cloud often blocks **inline SVG** rendering for security, so embed diagrams as PNG when you need them to display.
- For OAuth, request the scopes for every endpoint a command calls, including reads before writes. CQL `search` needs `search:confluence`, plus `read:confluence-content.all` for the page bodies it expands. Page-property `set`/`delete` need `read:page:confluence` and `write:page:confluence`; comment `update` needs `read:comment:confluence` and `write:comment:confluence`, while comment `delete` needs `delete:comment:confluence`. Page create also looks up its space; page update reads its current version unless `--base-version` is given; `children` needs `read:hierarchical-content:confluence`. Check the [property](https://developer.atlassian.com/cloud/confluence/rest/v2/api-group-content-properties/), [comment](https://developer.atlassian.com/cloud/confluence/rest/v2/api-group-comment/), and [page](https://developer.atlassian.com/cloud/confluence/rest/v2/api-group-page/) endpoint requirements. A `401 "scope does not match"` can indicate missing grants. Enable required scopes on the app, request them in the profile, then reauthorize; changing app permissions alone does not update an existing token.
- CQL compares user fields against account ids (`creator = "<accountId>"`); find them with `jira user search`.

## Pagination & output cheatsheet

| Need | Flag |
|---|---|
| One page | (default `--limit N`) |
| Every result | `--all` |
| Stream to disk / pipe | `--all --stream` (outputs JSONL) |
| Pick fields (Jira `get` & `search`) | `--fields key,summary,status` (or `*all` on `get` for the full issue) |
| Expand fields (Confluence `search` only) | `--expand ancestors,space` — `get` has no `--expand`; read `spaceId` from `get`, then map it via `space list` |

Jira `--all` requests 100 issues per page and ignores `--limit`. Confluence `--all` uses `--limit` for the first page only, capped at 50; subsequent requests follow the server's cursor links.

`--stream` writes JSONL to stdout and progress to stderr — never mix it with `--pretty`.

## Errors & exit codes

Runtime failures print a single-line JSON object to **stderr**; stdout stays results-only. CLI usage errors use text diagnostics and exit code `2`:

```json
{"error":{"message":"Failed to get issue (404 Not Found): ...","operation":"get issue","status":404}}
```

Parse `status`/`operation` instead of regexing the message; `hint` (present only when a known remediation exists, e.g. a 401 from an API token sent to the host that refuses its shape) is the remediation to relay to the user. Exit codes: `0` ok, `1` generic, `2` usage, `3` auth (401/403), `4` not found, `5` rate limited (429), `6` server error (5xx).

Reads and idempotent writes retry a 429 up to three times. Any other write returns it at once, because it may already have been applied: check whether it took effect (list the comments, search for the issue) before sending it again. An upload also returns at once because its body cannot be resent; running it again adds a new version of the file. A 429 error carries `retryAfterSeconds` and `rateLimitReason` when Atlassian sent them. Wait at least `retryAfterSeconds` before repeating anything, or, when it is absent, start at two seconds and double the wait each time. When the wait runs to minutes, stop and report it with `rateLimitReason` instead of retrying.

## Authentication

Use the configured profile. **Do not print secrets or invent credentials.**

The active profile dictates what identity the call runs as:

| profile method | who calls Atlassian |
|---|---|
| `oauth` | the signed-in human (token in OS keychain, auto-refreshed) |
| `scoped_token` | the API-token owner, limited to the scopes on the token (API token **with** scopes) |
| `basic` | the API-token owner, with their full permissions (classic/**unscoped** token) |
| `service_account` | a non-human service principal |

`basic` and `scoped_token` differ only in which host the token goes to, and each host accepts exactly one token shape. A 401 can mean an expired credential, a routing mismatch or insufficient scopes. Relay the `hint` and verify the relevant cause before changing configuration or asking the user to log in again.

Use `config validate` to diagnose Jira credentials. Its identity probe calls Jira, so a Confluence-only profile can fail that probe while remaining valid for Confluence. Diagnose that profile with a small Confluence read such as `confluence space list`. Do not replace credentials solely because an unrelated endpoint returned 401.

When the user reports auth trouble or asks to switch accounts:

```bash
atlassian-cli auth status                # expiry, scopes, storage backend
atlassian-cli auth refresh               # force token refresh (debugging)
atlassian-cli auth logout                # clears the stored session, whatever method the profile is on
```

`auth login` waits up to five minutes for a person to finish signing in through a browser, so ask the user to run `atlassian-cli auth login` in their own terminal rather than running it yourself. Without a browser on that machine they add `--no-browser` and open the printed URL in a browser that can reach the callback port; over SSH that means forwarding it first with `ssh -L 8976:127.0.0.1:8976 <host>`, with the profile's `redirect_port` in place of 8976 when one is set.

Switch profiles with `--profile <name>`; never invent profile names — list them via `atlassian-cli config list`. With an explicit `--config <file>`, `config list` includes that file and `config path/edit/init` target it. `--global` on path, edit or init selects the global file instead. `config show` emits the resolved profile as TOML with masked secrets; replace redactions before using it for authentication.

`config edit` opens an interactive editor and waits for it; edit the file `config path` prints instead. Operational settings omitted from a file inherit from earlier files; `[]` explicitly clears a list.

For the selected profile, the highest-priority file defining an `[auth]` section replaces that section entirely. Omit the section to preserve authentication from earlier files; credentials omitted inside it do not inherit from those files. Each credential then resolves as CLI flag > environment variable > selected file auth. When `ATLASSIAN_AUTH_METHOD` selects a different method, supply its credentials through CLI flags or environment variables.

## The installation itself

```bash
atlassian-cli self status                # version, binary path, skill state, profiles holding tokens — no network
atlassian-cli self update                # checks GitHub, replaces only after the download runs and reports the expected version
atlassian-cli self update --version 0.10.0  # a downgrade needs the version named; below 0.10.0 there is no `self` to come back with
atlassian-cli self skill install         # rewrite this skill from the running binary
```

The skill is compiled into the binary. If `self status` reports `stale`, the deployed copy differs from that binary; `self skill install` restores its bundled copy. `self update` changes nothing when the binary is already current. `self uninstall` and `self skill remove` remove things and take `--yes`; leave them to the user rather than running them on your own initiative.

## Behaviour worth knowing

- A `projects_filter` or `spaces_filter` on the active profile is auto-injected into bare JQL/CQL. If the user already names a project/space in their query, no second filter is added — write the query the user said.
- `jira search` uses `POST /rest/api/3/search/jql` under the hood — token-paginated, not offset.
