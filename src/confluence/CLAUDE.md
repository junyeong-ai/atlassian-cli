# confluence module

## v1 search + v2 everything else, plus three v1 writes

`search`/`search_all` call `/wiki/rest/api/search` (v1) because v2 has no CQL endpoint. Pages, comments, properties, spaces, and label/attachment *reads* use `/wiki/api/v2/*`. Three writes use v1 because v2 exposes no equivalent — do not move them to v2:

- **label add/remove** (`add_label`/`remove_label` → `POST`/`DELETE /wiki/rest/api/content/{id}/label`). The v2 Label group is GET-only. v1 `POST .../label` *adds* without clearing existing labels, so repeated calls are safe for agent retries; `remove_label` passes the label name via `.query(&[("name", …)])`, never the path.
- **attachment upload** (`upload_attachment` → `PUT /wiki/rest/api/content/{id}/child/attachment`). The PUT is multipart and upserts by filename (new file → create; existing → new version). It requires `X-Atlassian-Token: nocheck` to clear Confluence's XSRF check, and the reqwest `multipart` feature (enabled in `Cargo.toml`). The file is read with `std::fs::read` (tokio has no `fs` feature) and the display name is the path's final component. The part's `Content-Type` comes from `http_utils::content_type_for_filename` (a fixed extension→type table, the same one browsers use — a deterministic lookup, never byte sniffing) so images/PDFs render inline instead of becoming opaque `application/octet-stream` downloads; `--content-type` overrides it, and an unknown extension still falls back to `application/octet-stream`. Under OAuth this needs the `write:attachment:confluence` scope.

## Command surface (multi-op domains nested under an Action enum)

`comment`, `label`, `property`, `space`, `attachment` each route through a `Confluence*Action` enum, parallel to the Jira side. Function names follow the same verbs: `add_comment`/`update_comment`/`delete_comment`, `get_labels`/`add_label`/`remove_label`, `get_properties`/`set_property`/`delete_property`, `get_spaces`/`get_space`, `get_attachments`/`upload_attachment`.

- **Comments**: see *Comment threads* below for reads. `add_comment(page_id, body, parent_id, …)` — `parent_id = Some(id)` posts a threaded reply. The v2 endpoint requires *exactly one* container id, so a reply sends only `parentCommentId` and a top-level comment only `pageId` (sending both is a 400); both ride in the JSON body, never the path. `update_comment` bumps `version.number`, reading the current version via `fetch_version_number` first (same contract as `update_page`). Writes are footer-only — v2's inline-comment create needs an anchor (`inlineCommentProperties`) that has no CLI representation yet.
- **A key-filtered lookup is confirmed where its id is destroyed.** `fetch_property_by_key` reads the returned `key` back and refuses anything it cannot confirm — one that contradicts, and equally one that is absent, since the v2 schema does not require the field. Its id is what `set_property` overwrites and `delete_property` removes. `fetch_space_by_key` deliberately does not: `keys=` resolves space aliases, so a lookup by a historical alias legitimately returns a result naming neither the requested key nor the currently active one, and its id only decides where a page is created.
- **Deletes need no `--yes`**: `delete_comment`, `remove_label`, `delete_property` are id/name/key-scoped — the identifier is the specificity guard, matching the Jira `delete_comment`/`remove_link` family. Only whole-page `delete_page` requires `--yes`.
- **`search` items are heterogeneous.** A v1 result carrying a `content` object contributes that object; one that does not — v1 answers space and user entities too — contributes the result as it came, because dropping it reports no matches for a query that had them. So `.items[].id` resolves for content hits and not for the rest, and `count` is the number of matches, not the number of pages of content. `entityType`, which would otherwise tell them apart, is in `filter::DEFAULT_EXCLUDE_FIELDS`; presence of `id` is the discriminator a consumer has.
- **Response envelopes** (stable contract, matching the Jira side): lists return `{"items": [...]}`, creating writes return `{"id": ...}` via `response::require_field` (`create_page` adds the `title` the server settled on when the response carried one, which a caller may ignore — a `null` there would report a page named nothing) (a 2xx whose body lacks the id is schema drift and bails loudly rather than handing back a placeholder `null` a caller would chain on), and side-effect-only writes return `{}`.
- **`attachment upload`** sends `minorEdit` (the v1 endpoint expects it); the `--minor` flag sets it `true` to suppress the watcher notification a re-upload otherwise fires. The body part is `file`; its `Content-Type` is mapped from the extension (`--content-type` overrides) — see the attachment-write note above.

## Comment threads

v2 splits comments into two families — footer and inline — and addresses both through an identical path algebra: a page's roots (`/pages/{id}/{family}`), one comment's children (`/{family}/{id}/children`), one comment by id (`/{family}/{id}`). `CommentFamily` holds the differing segment so every comment read is one code path; do not branch on the family anywhere else.

**A page endpoint returns ROOT comments only.** The spec says so in as many words, and a reply is reachable solely through its parent's `children` collection. `ThreadWalk` therefore walks every level — each one a full `fetch_all_v2_results` cursor walk — and flattens the tree depth-first. Listing only the roots is the same failure the pagination helpers exist to prevent: an incomplete set handed back as a complete one.

Each emitted comment carries four fields the **walk** determined, never the response — so a row has the same shape at every depth:

| field | value |
|---|---|
| `location` | the family whose path was requested |
| `depth` | 0 at the roots of *this* listing, +1 per level |
| `parentCommentId` | the comment whose `children` collection this was read out of; explicit `null` at a root, so "answers nothing" and "not reported" stay distinguishable |
| `pageId` | the page this listing was opened on, at every depth: taking a root's from the response and a reply's from the walk would make one row's shape depend on what the server chose to include and the next row's not. Not stamped when the walk was opened on a comment id, where there is no page to name — whatever the response carries then stands |

`ThreadWalk` keeps a `seen` set of ids. A tree cannot reach a comment twice, so an id returning as its own descendant is drift and bails — the same posture `CursorTrail` takes toward a cursor that stops advancing. There is no depth cap; one would turn a legitimately deep thread into a failure. What `seen` cannot terminate is a server answering every `children` request with ids it has not used before, so `MAX_THREAD_COMMENTS` bounds the walk as a whole — the comments admitted plus the replies still queued, because each admitted comment can add a whole reply collection to the queue and bounding the admitted set alone would let it exhaust memory first. Set far beyond any real page, and failing loudly like `paginate`'s `MAX_PAGES` rather than truncating. It is a per-walk budget, so a listing covering both families carries one each.

**The family is part of a comment's address, not something to discover.** `get_comment`/`get_comment_replies` take it explicitly (CLI: `--location`, default `footer`). Retrying in the other family on a 404 would collapse "wrong family", "deleted" and "not visible to you" into one answer. Every comment this module emits carries its `location`, so an id it handed out is always complete.

Comment collections send `limit=250` (`COMMENT_PAGE_SIZE`) — the spec's maximum against a default of 25. Completeness comes from following `_links.next` either way, so the larger page is purely fewer round trips.

**`children_path` percent-encodes its comment id**, the worked example the root `CLAUDE.md` URL-safety rule names. It arrives in a *response* and is then interpolated into the path of the next request, so it is the one place where a response can choose where the walk goes next. Raw, an id of `../../../pages/999` resolves away and lands the request on `/wiki/pages/999/children`; a test pins the encoded form. Comment ids are numeric, so encoding is a no-op on real ones.

## Content properties = structured JSON metadata

`set_property` is a **key-scoped upsert**: it looks the key up via `fetch_property_by_key` (a `?key=` query on the collection), then `PUT`s with the bumped version when present or `POST`s when absent. `delete_property` resolves key→id the same way and errors on a missing key rather than silently succeeding. The CLI parses the `value` arg as **strict JSON** (no string-vs-JSON sniffing) — callers quote bare strings. `value` is arbitrary JSON, which is what makes properties a clean store for machine-read page state.

## Shared helpers (do not re-inline)

- `fetch_space_by_key(space_key)` is the single `/wiki/api/v2/spaces?keys=` lookup. It returns the **unfiltered** space object so callers diverge cleanly: `resolve_space_id` reads the raw `id` (a field filter must never strip it); `get_space` applies `filter::apply` before returning to the user.
- `fetch_version_number(client, url)` is the single "GET resource → read `version.number`", used by `update_page` and `update_comment`. It sends `include-version=true` (required by the page endpoint; the comment endpoint returns the version regardless).
- `fetch_all_v2_results` fetches every v2 list (see Pagination), and `v2_list_envelope` wraps the five plain ones. Add new list endpoints on top of them rather than writing a fresh GET/parse/envelope sequence; comments take the same fetch and their own envelope, for the reason under *Comment threads*.

## Body format

All body-bearing writes (`create_page`, `update_page`, `add_comment`, `update_comment`) send `body.representation = "storage"` with HTML in `body.value`. Storage format is Atlassian's canonical HTML dialect — accept HTML strings from callers and pass them through. Plain text is a valid storage document, so it is **not** auto-wrapped and there is **no** HTML-vs-text detection (unlike Jira ADF); the CLI is documented as HTML-in. This keeps the module heuristic-free.

Reads with `--format markdown` convert `body.storage.value` via `markdown::confluence_to_markdown`. The JSON envelope is preserved; only the HTML string field is replaced.

## Pagination

Every page request is issued as a **service-relative path** through `client.get(Service::Confluence, …)`. `link_path` normalizes the `_links` value first, so the host always comes from `build_url` (local config) and never from the response — see the root `CLAUDE.md` note on why that matters under the token methods. `build_url` appends the path verbatim, so an embedded cursor query survives intact under both direct-domain and proxy auth.

`link_path` accepts a link only if it is rooted at the site (`/…`) or absolute (host discarded, path kept), and returns `None` for anything else. Two details are easy to get wrong and are pinned by tests:

- **Test the leading `/` before looking for a scheme.** Confluence does not percent-encode the `cql` it echoes into `next`, so searching for a URL yields a rooted link that contains `://`. Classifying on `contains("://")` first mangles that link and drops its cursor.
- **A scheme-less link is not automatically a path.** `@evil.example/…` has no scheme and no leading `/`; appending it to the configured origin hands the request to `evil.example`. Reject rather than pass through.

`fetch_all_v2_results` also stops at `MAX_CURSOR_PAGES`, the counterpart to `paginate`'s `MAX_PAGES`: a server handing out a genuinely new cursor every time repeats nothing and stalls nothing, so the trail below cannot end that walk.

Both walks record their steps in a `CursorTrail`, which resolves each link and refuses one that repeats a path already fetched. It keys on the resolved path, not the raw link, so a server that varies only the discarded host cannot keep the walk going. A link with no usable path, and a cursor that stops advancing, are both schema drift: they bail, matching the module's refusal to return a truncated list.

The two API generations disagree on what `next` is relative to, which is the only reason there are two mechanisms:

- **v1 search** (`search_all`): `_links.base` is `…/wiki` and `_links.next` is `/rest/api/search?…` — relative to the *base path*. `v1_next_link` joins the two into an absolute link, which `link_path` then reduces. Resolving `next` as host-root-relative (what a standards-conformant URL join does) would drop `/wiki` and 404.
- **v2 lists** (`fetch_all_v2_results`): `_links.next` is already rooted at the site (`/wiki/api/v2/…`), so it is used as-is. Joining it onto `base` would duplicate `/wiki`. Every v2 list endpoint (`get_page_children`, `get_labels`, `get_properties`, `get_spaces`, `get_attachments`, and every level of a comment thread) funnels through this helper, which follows `next` to exhaustion so a single page is never silently returned as the whole set. The per-call `query` (e.g. a comment collection's `body-format=storage&limit=250`) is sent on the **first** request only — each `next` link already carries it forward. The five plain lists are wrapped and filtered by `v2_list_envelope`, which filters the items and never the envelope — the `{"items": [...]}` wrapper is this tool's contract rather than a field of the response, so `response_exclude_fields` cannot reach it. Comment listings go through `comment_envelope` instead, which does not filter: `ThreadWalk::admit` has already filtered each comment before stamping `location`, `depth`, `parentCommentId` and `pageId`, and filtering again would take exactly those — `location` is what makes an id this module hands out a complete address.

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

Every site that interpolates an identifier into a `/wiki/...` path segment
goes through `http_utils::encode_path_segment` — the caller's `page_id` and
equally a comment or property id the API returned, since a response choosing
the next request's path is the position the encoder exists for. The encoder is
RFC 3986 strict (brackets, `:`, `@`, slash, etc. are all percent-encoded).

A pagination `_links.next` is not an identifier and does not go through it: it
is a whole path with a cursor query, and encoding would corrupt the cursor. It
reaches `build_url` as an already-formed path via `link_path`, which is what
checks it.
