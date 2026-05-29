# confluence module

## v1 search + v2 everything else

`search`/`search_all` call `/wiki/rest/api/search` (v1) because v2 has no CQL endpoint. Page/space/comment operations use `/wiki/api/v2/*`. Do not attempt to unify them.

## Body format

All writes (`create_page`, `update_page`) send `body.representation = "storage"` with HTML in `body.value`. Storage format is Atlassian's canonical HTML dialect — accept HTML strings from callers and pass them through. Plain text is not auto-wrapped here (unlike Jira ADF); the CLI is documented as HTML-in.

Reads with `--format markdown` convert `body.storage.value` via `markdown::confluence_to_markdown`. The JSON envelope is preserved; only the HTML string field is replaced.

## Pagination — two-stage URL

`search_all` fetches the first page via `client.get(Service::Confluence, "/wiki/rest/api/search")`, then follows `_links.next` (a relative path) combined with `_links.base` (a URL that may point at the original atlassian.net host under service account auth).

The combined URL must go through `client.rewrite_url(Service::Confluence, &url)` before `client.get_absolute(...)` — without rewriting, service account requests hit the wrong host and fail auth. Basic auth leaves the URL unchanged.

## `children` has no markdown format

v2 `/wiki/api/v2/pages/{id}/children` returns metadata only (no body), so the `--format` flag is intentionally absent on `confluence children`.

## Space filter injection

`apply_space_filter` parallels the JQL project-filter logic in `jira/api.rs`:
detect a user-written `space` clause with the `SPACE_CLAUSE_RE` word-boundary
regex applied to `query_utils::mask_string_literals(cql)`. Masking blanks
the contents of every `"…"` literal before matching, so a query like
`title ~ "deep space"` does not suppress the configured filter and an
identifier like `mySpace = X` does not false-positive on the `space`
keyword. When the regex misses, the original CQL is wrapped with
`space IN ("S1","S2") AND (…)` — match the Jira-side pattern exactly so
future query-language additions inherit the same defense.

## URL path encoding

Every site that interpolates `page_id` (or any other user-controllable
identifier) into a `/wiki/...` path goes through
`http_utils::encode_path_segment`. The encoder is RFC 3986 strict
(brackets, `:`, `@`, slash, etc. are all percent-encoded). Server-side
identifiers returned by the API — including the `_links.base` host in
pagination responses — must NOT be encoded; they pass through
`client.rewrite_url` unchanged.
