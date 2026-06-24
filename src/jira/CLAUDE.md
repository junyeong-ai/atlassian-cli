# jira module

## ADF input normalization

Every write path that accepts a description or comment body routes through `jira::adf::process_description_input` or `process_comment_input`:

- `Value::String` → **total**: a string that parses to a valid ADF object is used as-is; everything else (plain prose, or JSON-object-shaped text like `{"status":"done"}` that isn't valid ADF) is wrapped in a minimal ADF `doc`. String input never errors, so any text can be posted verbatim.
- `Value::Object` that is valid ADF → passed through after `validate_adf`; an invalid object `bail!`s (an explicit object unambiguously intends ADF).
- `Value::Null` → treated as an empty ADF doc (not an error; users send `null` when clearing fields).
- Anything else (number, bool, array) → `bail!` with the field name in the message.

When adding a new write endpoint that takes user text, route through these helpers rather than sending the raw string. Do not accept plain strings at the API boundary without conversion — Jira's v3 API rejects them.

## Search field selection

`fields::resolve_search_fields` is the only place that decides which fields go into `/search/jql`. Precedence: CLI `--fields` > `[jira].search_default_fields` / `JIRA_SEARCH_DEFAULT_FIELDS` env > the baseline list in `fields.rs`, plus `search_custom_fields` appended. When `--format markdown` is set, `description` is added (otherwise omitted for response size).

## Response envelope

- `search` returns `{"items": [...], "count": N}` — `count` is the size of this page.
- `search_all` returns `{"items": [...], "total": N}` — `total` is the cumulative size (`/search/jql` doesn't expose a server-side total).
- `get_issue` returns the raw Jira issue object after `filter::apply`. `--fields` selects the returned fields (default: the essential set plus configured custom fields; `*all` returns the full issue); it is not capped by a hardwired whitelist.
- All list endpoints (`get_comments`, `get_transitions`, `get_links`, `get_link_types`, `get_worklogs`, `get_watchers`, `get_issue_types`, `get_priorities`, `get_statuses`, `get_labels`, `get_boards`, `get_sprints`) return `{"items": [...]}`.
- `create_issue` returns `{"key": ..., "id": ...}` — both fields are stable contract; `key` is the human-readable identifier needed for follow-up commands.
- Writes that target an identifiable issue sub-resource (`add_comment`, `update_comment`, `add_worklog`, `update_worklog`) return `{"id": ...}` so callers can chain follow-up updates without re-querying.
- Every id/key/version returned by a write is pulled through `response::require_field`: a 2xx whose body lacks the field bails loudly instead of returning a placeholder `null`, so a caller never chains on a non-existent resource.
- Side-effect-only writes return `{}` (`update_issue`, `delete_issue`, `delete_comment`, `transition_issue`, `add_link`, `remove_link`, `remove_worklog`, `add_watcher`, `remove_watcher`, `move_issues_to_sprint`, `move_issues_to_backlog`, `assign_issues_to_epic`, `unassign_issues_from_epic`).

Keep these shapes stable — downstream tooling (skill, scripts) depends on them.
