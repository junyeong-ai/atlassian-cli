# confluence module

## v1 search + v2 everything else, plus three v1 writes

`search`/`search_all` call `/wiki/rest/api/search` (v1) because v2 has no CQL endpoint. Pages, comments, properties, spaces, and label/attachment *reads* use `/wiki/api/v2/*`. Three writes use v1 because v2 exposes no equivalent — do not move them to v2:

- **label add/remove** (`add_label`/`remove_label` → `POST`/`DELETE /wiki/rest/api/content/{id}/label`). The v2 Label group is GET-only. v1 `POST .../label` *adds* without clearing existing labels, so repeated calls are safe for agent retries; `remove_label` passes the label name via `.query(&[("name", …)])`, never the path.
- **attachment upload** (`upload_attachment` → `PUT /wiki/rest/api/content/{id}/child/attachment`). The PUT is multipart and upserts by filename (new file → create; existing → new version). It requires `X-Atlassian-Token: nocheck` to clear Confluence's XSRF check, and the reqwest `multipart` feature (enabled in `Cargo.toml`). The file is read with `std::fs::read` (tokio has no `fs` feature) and the display name is the path's final component — deterministic parsing, not content sniffing. Under OAuth this needs the `write:attachment:confluence` scope.

## Command surface (multi-op domains nested under an Action enum)

`comment`, `label`, `property`, `space`, `attachment` each route through a `Confluence*Action` enum, parallel to the Jira side. Function names follow the same verbs: `add_comment`/`update_comment`/`delete_comment`, `get_labels`/`add_label`/`remove_label`, `get_properties`/`set_property`/`delete_property`, `get_spaces`/`get_space`, `get_attachments`/`upload_attachment`.

- **Footer comments**: `add_comment(page_id, body, parent_id, …)` — `parent_id = Some(id)` posts a threaded reply via `parentCommentId`. `pageId`/`parentCommentId` ride in the JSON body (no path encoding). `update_comment` bumps `version.number`, reading the current version via `fetch_version_number` first (same contract as `update_page`).
- **Deletes need no `--yes`**: `delete_comment`, `remove_label`, `delete_property` are id/name/key-scoped — the identifier is the specificity guard, matching the Jira `delete_comment`/`remove_link` family. Only whole-page `delete_page` requires `--yes`.
- **Response envelopes** (stable contract, matching the Jira side): lists return `{"items": [...]}`, creating writes return `{"id": ...}` (the API's contractually-present id, passed through — a malformed 2xx degrades to `{"id": null}` rather than a *false* failure, since the write did succeed), and side-effect-only writes return `{}`.
- **`attachment upload`** sends `minorEdit` (the v1 endpoint expects it); the `--minor` flag sets it `true` to suppress the watcher notification a re-upload otherwise fires. The body part is `file`; mime defaults to `application/octet-stream` (no extension sniffing).

## Content properties = structured JSON metadata

`set_property` is a **key-scoped upsert**: it looks the key up via `fetch_property_by_key` (a `?key=` query on the collection), then `PUT`s with the bumped version when present or `POST`s when absent. `delete_property` resolves key→id the same way and errors on a missing key rather than silently succeeding. The CLI parses the `value` arg as **strict JSON** (no string-vs-JSON sniffing) — callers quote bare strings. `value` is arbitrary JSON, which is what makes properties a clean store for machine-read page state.

## Shared helpers (do not re-inline)

- `fetch_space_by_key(space_key)` is the single `/wiki/api/v2/spaces?keys=` lookup. It returns the **unfiltered** space object so callers diverge cleanly: `resolve_space_id` reads the raw `id` (a field filter must never strip it); `get_space` applies `filter::apply` before returning to the user.
- `fetch_version_number(client, url)` is the single "GET resource → read `version.number`", used by `update_page` and `update_comment`. It sends `include-version=true` (required by the page endpoint; the comment endpoint returns the version regardless).
- `fetch_all_v2_results` + `v2_list_envelope` back every v2 list endpoint (see Pagination). Add new list endpoints on top of them rather than writing a fresh GET/parse/envelope sequence.

## Body format

All body-bearing writes (`create_page`, `update_page`, `add_comment`, `update_comment`) send `body.representation = "storage"` with HTML in `body.value`. Storage format is Atlassian's canonical HTML dialect — accept HTML strings from callers and pass them through. Plain text is a valid storage document, so it is **not** auto-wrapped and there is **no** HTML-vs-text detection (unlike Jira ADF); the CLI is documented as HTML-in. This keeps the module heuristic-free.

Reads with `--format markdown` convert `body.storage.value` via `markdown::confluence_to_markdown`. The JSON envelope is preserved; only the HTML string field is replaced.

## Pagination

Two cursor-following mechanisms, split by API generation:

- **v1 search** (`search_all`): fetches the first page via `client.get(Service::Confluence, "/wiki/rest/api/search")`, then follows `_links.next` (relative) combined with `_links.base`. The combined URL goes through `client.rewrite_url` before `client.get_absolute(...)` — without rewriting, service-account requests hit the wrong host.
- **v2 lists** (`fetch_all_v2_results`): every v2 list endpoint (`get_comments`, `get_page_children`, `get_labels`, `get_properties`, `get_spaces`, `get_attachments`) funnels through this helper. It follows `_links.next` to exhaustion so a single page is never silently returned as the whole set. A relative `next` (`/wiki/…`) is re-issued via `client.get(Service::Confluence, next)` — `build_url` is plain string concatenation, so the embedded cursor query survives intact under both direct-domain and proxy auth; an absolute `next` takes the `rewrite_url` + `get_absolute` path. The per-call `query` (e.g. `get_comments`'s `body-format=storage`) is sent on the **first** request only — each `next` link already carries it forward. Results are wrapped and filtered once via `v2_list_envelope`.

Do not re-inline a one-page GET for any v2 list — silent truncation past the first page is exactly what this helper exists to prevent.

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
