# jira module

`api.rs` owns shared pagination, filtering and metadata; `api/issues.rs`,
`comments.rs`, `links.rs`, `worklogs.rs` and `agile.rs` own their operations.
Public functions remain exported through `jira::api` and `jira`.

## ADF input normalization

Description and comment bodies normalize through `jira::adf::process_description_input` or `process_comment_input`:

- `Value::String` → a string that parses to an object passing `validate_adf` is used as-is; other strings, including JSON-shaped prose like `{"status":"done"}`, are wrapped in a minimal ADF `doc`. Normalization accepts every string; the API can still reject nested ADF content.
- `Value::Object` → passed through after `validate_adf`; an object failing that check `bail!`s.
- `Value::Null` → the normalization helper returns an empty ADF paragraph.
- Anything else (number, bool, array) → `bail!` with the field name in the message.

`validate_adf` checks only the root: `type = "doc"`, integer `version = 1`, and an array `content`. Jira validates the nested node schema.

Empty strings become empty ADF paragraphs without empty text nodes; whitespace is preserved. Issue updates send an explicit `description: null` unchanged to clear the field. An omitted description is not sent and leaves it unchanged.

When adding a new write endpoint that takes user text, route through these helpers rather than sending the raw string. Do not accept plain strings at the API boundary without conversion — Jira's v3 API rejects them.

Under `--format markdown` the command layer converts a body, or a string `description` among JSON fields, with `markdown::markdown_to_adf` first, so these helpers receive an ADF object and validate its root.

## Search field selection

`fields::resolve_search_fields` decides the search fields. An explicit `--fields` list is returned unchanged. Otherwise configured `search_default_fields` replace the built-in defaults, and Markdown mode adds `description`. `search_custom_fields` extend only the built-in defaults; they do not extend an explicit or configured replacement list.

## Response envelope

- `search` returns `{"items": [...], "count": N}` — `count` is the size of this page.
- `search_all` returns `{"items": [...], "total": N}` — `total` is the cumulative size (`/search/jql` doesn't expose a server-side total).
- `get_issue` returns the raw Jira issue object after `filter::apply`. `--fields` selects the returned fields (default: the essential set plus configured custom fields; `*all` returns the full issue); it is not capped by a hardwired whitelist.
- All list endpoints (`get_comments`, `get_transitions`, `get_links`, `get_link_types`, `get_worklogs`, `get_watchers`, `get_issue_types`, `get_priorities`, `get_statuses`, `get_labels`, `search_users`, `get_boards`, `get_sprints`) return `{"items": [...]}`.
- `create_issue` returns `{"key": ..., "id": ...}` — both fields are stable contract; `key` is the human-readable identifier needed for follow-up commands.
- Writes that target an identifiable issue sub-resource (`add_comment`, `update_comment`, `add_worklog`, `update_worklog`) return `{"id": ...}` so callers can chain follow-up updates without re-querying.
- Every id/key/version returned by a write is pulled through `response::require_field`: a 2xx whose body lacks the field bails loudly instead of returning a placeholder `null`, so a caller never chains on a non-existent resource.
- Side-effect-only writes return `{}` (`update_issue`, `delete_issue`, `delete_comment`, `transition_issue`, `add_link`, `remove_link`, `remove_link_by_id`, `remove_worklog`, `add_watcher`, `remove_watcher`, `move_issues_to_sprint`, `move_issues_to_backlog`, `assign_issues_to_epic`, `unassign_issues_from_epic`).

Keep these shapes stable — downstream tooling (skill, scripts) depends on them.

## Pagination contracts

`paginate` is the single `startAt`/`maxResults` loop. Which fields it reads comes from the `PageContract` a caller names:

- `AGILE_PAGE` — `values` + `isLast`. `/rest/api/3/label`, `/rest/agile/1.0/board`, sprints.
- `COMMENT_PAGE` — `comments` + `total`. `/rest/api/3/issue/{key}/comment`, which reports how many comments exist rather than flagging the last page.
- `WORKLOG_PAGE` — `worklogs` + `total`. `/rest/api/3/issue/{key}/worklog`, on the same contract.

Under `Total` the count is the whole contract, so a page that adds nothing while items remain is the server declining to hand them over — that bails, where the `IsLast` arm's empty page is a legitimate skip-ahead. A response missing its items array, its `isLast`, or its `total` bails too: a truncated collection returned as a complete one is the failure this helper exists to prevent.

Add a new endpoint by adding a `PageContract` constant, never a second loop.

`search_all` sits outside this helper because `/search/jql` is token-paginated rather than offset-paginated; it carries its own loop and its own bound. Everything else outside it is not paginated at all — `/issue/{key}/watchers`, `/issue/{key}` for links, and `/issue/{key}/transitions` each answer with a whole set, and the three discovery reads (`/issuetype`, `/priority`, `/status`) answer with their list and no envelope around it, where `require_array(&data, WHOLE_BODY, …)` is what holds the `{"items": [...]}` contract.

Every walk takes its items and its end signal off the response **before** `filter::apply` sees it. `response_exclude_fields` is the caller's list, so a name in it matching a control field would delete the answer the loop reads — and a walk that concluded from that deletion would hand back its first page as the whole result. Filtering is per item, which is all it was ever for.
