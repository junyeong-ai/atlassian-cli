# jira module

## ADF input normalization

Every write path that accepts a description or comment body routes through `jira::adf::process_description_input` or `process_comment_input`:

- `Value::String("plain")` → wrapped in a minimal ADF `doc`.
- `Value::Object` that is valid ADF → passed through after `validate_adf`.
- `Value::Null` → treated as an empty ADF doc (not an error; users send `null` when clearing fields).
- Anything else → `bail!` with the field name in the message.

When adding a new write endpoint that takes user text, route through these helpers rather than sending the raw string. Do not accept plain strings at the API boundary without conversion — Jira's v3 API rejects them.

## Search field selection

`fields::resolve_search_fields` is the only place that decides which fields go into `/search/jql`. Precedence: CLI `--fields` > `[jira].search_default_fields` / `JIRA_SEARCH_DEFAULT_FIELDS` env > the baseline list in `fields.rs`, plus `search_custom_fields` appended. When `--format markdown` is set, `description` is added (otherwise omitted for response size).

## Response envelope

- `search` returns `{"items": [...], "count": N}` — `count` is the size of this page.
- `search_all` returns `{"items": [...], "total": N}` — `total` is the cumulative size (`/search/jql` doesn't expose a server-side total).
- `get_*` (issue / comments / transitions) return the raw Jira object after `filter::apply`.

Keep these shapes stable — downstream tooling (skill, scripts) depends on them.
