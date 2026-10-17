use crate::client::{ApiClient, Service, extract_path_and_query};
use crate::config::Config;
use crate::confluence::fields::build_search_expand;
use crate::filter;
use crate::http_utils::encode_path_segment;
use crate::markdown::confluence_to_markdown;
use crate::query_utils::inject_filter;
use crate::response::{require_field, require_u64};
use anyhow::Result;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::io::{self, Write};
use std::time::Duration;
use tokio::time::sleep;

mod pages;
pub use pages::{create_page, delete_page, get_page, get_page_children, update_page};
mod content;
pub use content::{
    add_label, delete_property, get_attachments, get_labels, get_properties, remove_label,
    set_property, upload_attachment,
};
mod spaces;
pub use spaces::{get_space, get_spaces, resolve_space_id};

/// Operative page-size cap for the CQL search endpoint. The v1 search with
/// body expansion (which we always request) is throttled well below the
/// non-body ceiling, so 50 is the real maximum a single page returns.
const MAX_SEARCH_LIMIT: u32 = 50;

fn apply_space_filter(cql: &str, config: &Config) -> String {
    if config.confluence.spaces_filter.is_empty() {
        return cql.to_string();
    }

    let spaces = config
        .confluence
        .spaces_filter
        .iter()
        .map(|s| format!("\"{}\"", s))
        .collect::<Vec<_>>()
        .join(",");

    inject_filter(cql, "space", &format!("space IN ({})", spaces))
}

/// Clamp a user-requested page size to `MAX_SEARCH_LIMIT`. Shared by
/// single-page `search` and the first page of `search_all` so both interpret
/// `--limit` identically.
fn effective_search_limit(limit: u32) -> u32 {
    limit.clamp(1, MAX_SEARCH_LIMIT)
}

/// Reduce a server-supplied pagination link to a site-rooted path.
///
/// Two shapes are usable: a link already rooted at the site (`/wiki/…`), and an
/// absolute link, whose path is kept and whose host is discarded. Anything else
/// is rejected. A link must prove it is a path, because the request built from
/// it carries the caller's credentials and `build_url` appends it to the
/// configured origin.
///
/// Rootedness is decided before anything else. Confluence echoes `cql` into a
/// `next` link without percent-encoding it, so searching for a URL yields a
/// genuinely rooted link that contains `://`; reading "absolute" out of that
/// substring would discard the real path along with the cursor.
fn link_path(link: &str) -> Option<&str> {
    if link.starts_with('/') {
        return Some(link);
    }
    extract_path_and_query(link).filter(|path| path.starts_with('/'))
}

/// The paths a cursor walk has already requested.
///
/// A cursor that resolves to a page already fetched is not advancing, and
/// following it would loop until the server stopped answering. Membership is
/// keyed on the resolved path because that is what determines the request —
/// keying on the raw link would let a server vary only the discarded host and
/// walk forever.
#[derive(Default)]
struct CursorTrail(HashSet<String>);

impl CursorTrail {
    /// Resolve `link` and record it as the next step of the walk.
    ///
    /// Errors when the link carries no usable path, or when it revisits one.
    /// Neither is the end of the collection, so neither may pass for one: a
    /// short list returned as if complete is the failure this walk exists to
    /// prevent.
    fn step(&mut self, what: &str, link: &str) -> Result<String> {
        let path = link_path(link)
            .ok_or_else(|| {
                anyhow::anyhow!("Failed to {what}: pagination link had no path: {link}")
            })?
            .to_string();
        if !self.0.insert(path.clone()) {
            anyhow::bail!("Failed to {what}: pagination cursor did not advance");
        }
        Ok(path)
    }
}

/// Join a v1 search `_links` envelope into the link of its next page.
///
/// The two API generations disagree on what `next` is relative to. v1 states it
/// against `_links.base`, whose path carries the `/wiki` prefix the request
/// needs (`base` = `…/wiki`, `next` = `/rest/api/search?…`); v2 states it from
/// the site root (`/wiki/api/v2/…`), already complete. Only v1 joins, and only
/// when `next` is rooted — an absolute `next` is its own answer.
///
/// The result is a link, not a path: `link_path` still reduces it, so a `base`
/// naming a foreign host contributes nothing but its path.
fn v1_next_link(links: &Value, next: &str) -> String {
    match links["base"].as_str() {
        Some(base) if next.starts_with('/') => format!("{base}{next}"),
        _ => next.to_string(),
    }
}

pub async fn search(
    query: &str,
    limit: u32,
    include_all_fields: Option<bool>,
    additional_expand: Option<Vec<String>>,
    as_markdown: bool,
    client: &ApiClient,
) -> Result<Value> {
    let final_cql = apply_space_filter(query, client.config());
    let url = "/wiki/rest/api/search";
    let expand = build_search_expand(include_all_fields, additional_expand);

    let effective_limit = effective_search_limit(limit);

    let request = client
        .get(Service::Confluence, url)
        .await?
        .header("Accept", "application/json")
        .query(&[
            ("cql", final_cql.as_str()),
            ("limit", &effective_limit.to_string()),
            ("expand", &expand),
        ]);
    let response = client.execute("search", request).await?;

    let mut data: Value = response.json().await?;

    let mut items = extract_content_from_results(&mut data, as_markdown)?;
    let total = require_u64(&data, "/totalSize", "search")?;
    // As on the `--all` path and the Jira side: the items are filtered, the
    // envelope this CLI promises is not.
    for item in &mut items {
        filter::apply(item, client.config());
    }

    Ok(json!({
        "items": items,
        "count": items.len(),
        "total": total,
    }))
}

pub async fn search_all(
    query: &str,
    limit: u32,
    include_all_fields: Option<bool>,
    additional_expand: Option<Vec<String>>,
    stream: bool,
    as_markdown: bool,
    client: &ApiClient,
) -> Result<Value> {
    let final_cql = apply_space_filter(query, client.config());
    let expand = build_search_expand(include_all_fields, additional_expand);

    // The counterpart of `fetch_all_v2_results`' ceiling, and for the same
    // reason: `CursorTrail` refuses a repeated path, and a server handing out a
    // genuinely new cursor every time repeats nothing, so nothing else ends
    // that walk. Counted in requests rather than results because that is what
    // runs away — which does mean the results a walk can return are this
    // many times its page size, and `limit` is the caller's. At the default 50
    // that is half a million; a caller who lowered it to keep each request
    // small is the one who can hit this, so the failure names the page size it
    // was reached at.
    const MAX_PAGES: u32 = 10_000;
    let mut all_items: Vec<Value> = Vec::new();
    let mut total = 0usize;
    let mut next_url: Option<String> = None;
    let mut total_size: u64 = 0;
    let mut trail = CursorTrail::default();
    let mut finished = false;

    for page_num in 1..=MAX_PAGES {
        let mut data = if let Some(ref url) = next_url {
            fetch_page(client, url).await?
        } else {
            fetch_initial_page(client, &final_cql, &expand, limit).await?
        };

        if page_num == 1 {
            // Progress-only: feeds the stderr "fetched X/Y" line. The returned
            // envelope's `total` is the exact fetched count, so
            // unlike single-page `search` this estimate is non-load-bearing and
            // a missing `totalSize` degrades to 0 rather than aborting the crawl.
            total_size = data["totalSize"].as_u64().unwrap_or(0);
        }

        let mut items = extract_content_from_results(&mut data, as_markdown)?;
        // Apply response filtering per item so `--all` output matches the
        // single-page `search` envelope. Done before streaming so streamed
        // and accumulated items are filtered identically.
        for item in &mut items {
            filter::apply(item, client.config());
        }
        let count = items.len();

        if stream {
            for item in &items {
                println!("{}", serde_json::to_string(item)?);
            }
            io::stdout().flush()?;
        }

        total += count;
        if !stream {
            all_items.extend(items);
        }

        eprintln!(
            "  Page {}: {} items (fetched: {}/{})",
            page_num, count, total, total_size
        );

        // Absent or null is the API's end-of-results signal; a `next` that is
        // there and is not a string is drift, the same distinction `link_path`
        // draws one step later. An empty page is neither — with `next` still
        // live, breaking on one drops every page after it.
        let next = &data["_links"]["next"];
        if next.is_null() {
            finished = true;
            break;
        }
        let Some(link) = next.as_str() else {
            anyhow::bail!("search succeeded but its '_links.next' was not a string: {next}");
        };

        next_url = Some(trail.step("search", &v1_next_link(&data["_links"], link))?);

        sleep(Duration::from_millis(
            client.config().performance.rate_limit_delay_ms,
        ))
        .await;
    }

    if !finished {
        let page_size = effective_search_limit(limit);
        // Only where there is room to raise it. At the cap the advice would be
        // an instruction the caller has already followed.
        let remedy = if page_size < MAX_SEARCH_LIMIT {
            format!(" — raise --limit, up to {MAX_SEARCH_LIMIT}, to cover more per page")
        } else {
            String::new()
        };
        anyhow::bail!(
            "search did not finish within {MAX_PAGES} pages of {page_size} results{remedy}"
        );
    }

    eprintln!("\nTotal: {} items fetched", total);

    // See `jira::search_all` — Null signals `output_json` to skip, so the
    // trailing summary doesn't pollute the JSONL stream.
    if stream {
        Ok(Value::Null)
    } else {
        Ok(json!({
            "items": all_items,
            "total": total
        }))
    }
}

async fn fetch_initial_page(
    client: &ApiClient,
    cql: &str,
    expand: &str,
    limit: u32,
) -> Result<Value> {
    let url = "/wiki/rest/api/search";
    let effective_limit = effective_search_limit(limit).to_string();

    let request = client
        .get(Service::Confluence, url)
        .await?
        .header("Accept", "application/json")
        .query(&[
            ("cql", cql),
            ("limit", &effective_limit),
            ("expand", expand),
        ]);
    let response = client.execute("search", request).await?;

    response.json().await.map_err(Into::into)
}

async fn fetch_page(client: &ApiClient, path: &str) -> Result<Value> {
    let request = client
        .get(Service::Confluence, path)
        .await?
        .header("Accept", "application/json");
    let response = client.execute("search", request).await?;

    response.json().await.map_err(Into::into)
}

/// The two comment families Confluence v2 exposes.
///
/// Both are addressed through an identical path algebra — a page's roots, one
/// comment's children, one comment by id — differing only in a path segment.
/// Carrying that segment as data keeps every comment operation on one code path
/// instead of two that drift, and makes the family part of a comment's address
/// rather than something to infer from a request that failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CommentFamily {
    Footer,
    Inline,
}

impl CommentFamily {
    /// Both families, in the order a combined listing reports them.
    pub const ALL: [CommentFamily; 2] = [CommentFamily::Footer, CommentFamily::Inline];

    /// The path segment, and the value stamped onto every comment emitted from
    /// this family — one table, so the two can never name different things.
    const fn parts(self) -> (&'static str, &'static str) {
        match self {
            CommentFamily::Footer => ("footer-comments", "footer"),
            CommentFamily::Inline => ("inline-comments", "inline"),
        }
    }

    fn segment(self) -> &'static str {
        self.parts().0
    }

    fn label(self) -> &'static str {
        self.parts().1
    }

    fn page_path(self, page_id: &str) -> String {
        format!(
            "/wiki/api/v2/pages/{}/{}",
            encode_path_segment(page_id),
            self.segment()
        )
    }

    fn children_path(self, comment_id: &str) -> String {
        format!(
            "/wiki/api/v2/{}/{}/children",
            self.segment(),
            encode_path_segment(comment_id)
        )
    }

    fn single_path(self, comment_id: &str) -> String {
        format!(
            "/wiki/api/v2/{}/{}",
            self.segment(),
            encode_path_segment(comment_id)
        )
    }
}

/// Page size for every comment collection. The endpoints default to 25 and cap
/// at 250; completeness comes from following `_links.next` either way, so the
/// larger page is purely fewer round trips.
const COMMENT_PAGE_SIZE: &str = "250";

/// Hard ceiling on one thread walk, so a server answering every `children`
/// request with fresh ids cannot make it run forever. Far beyond any real
/// page — the largest discussions on a wiki run to hundreds — so it fails
/// loudly rather than truncating, matching `paginate`'s `MAX_PAGES`.
const MAX_THREAD_COMMENTS: usize = 50_000;

fn comment_query() -> [(&'static str, &'static str); 2] {
    [("body-format", "storage"), ("limit", COMMENT_PAGE_SIZE)]
}

/// Depth-first accumulation of a comment tree, one cursor walk per level.
///
/// A page endpoint returns only ROOT comments, and a reply is reachable solely
/// through its parent's `children` collection — so a listing that stops at the
/// roots hands back an incomplete thread as if it were the whole one. Every
/// level goes through `fetch_all_v2_results`, which is also what keeps a long
/// reply chain from being cut at a page boundary.
///
/// `seen` is what makes the walk terminate on a repeat: a tree cannot reach a
/// comment twice, so an id that returns as its own descendant is drift, and
/// drift bails rather than looping — the same posture `CursorTrail` takes
/// toward a cursor that stops advancing. `limit` covers the case `seen` cannot,
/// a server answering every level with fresh ids forever.
struct ThreadWalk<'a> {
    client: &'a ApiClient,
    family: CommentFamily,
    /// Stamped onto every comment in the listing. `None` where the walk began
    /// at a comment rather than a page, and the container is therefore not
    /// known here.
    page_id: Option<&'a str>,
    include_replies: bool,
    seen: HashSet<String>,
    limit: usize,
}

impl<'a> ThreadWalk<'a> {
    fn new(
        client: &'a ApiClient,
        family: CommentFamily,
        page_id: Option<&'a str>,
        include_replies: bool,
    ) -> Self {
        ThreadWalk {
            client,
            family,
            page_id,
            include_replies,
            seen: HashSet::new(),
            limit: MAX_THREAD_COMMENTS,
        }
    }

    /// Flatten `roots` and everything below them, depth first.
    ///
    /// `root_parent` is the comment the roots answer, which the caller knows
    /// and the root objects do not carry: a page's roots answer nothing, a
    /// `replies` listing's roots answer the comment that was asked for.
    async fn collect(
        &mut self,
        what: &str,
        roots: Vec<Value>,
        root_parent: Option<&str>,
    ) -> Result<Vec<Value>> {
        let mut out = Vec::with_capacity(roots.len());
        // Reversed on push so each level pops in the order the server returned
        // it, and a comment is emitted before the replies below it.
        let mut pending: Vec<(Value, Option<String>, usize)> = roots
            .into_iter()
            .rev()
            .map(|comment| (comment, root_parent.map(str::to_string), 0))
            .collect();

        while let Some((mut comment, parent, depth)) = pending.pop() {
            // Counted together, because they grow at different rates: each
            // admitted comment adds one to `seen` and up to a page of replies
            // to `pending`, so bounding the admitted set alone would let the
            // queue exhaust memory long before the ceiling was reached.
            if self.seen.len() + pending.len() >= self.limit {
                anyhow::bail!(
                    "Failed to {what}: more than {} comments in one thread walk — aborting to \
                     avoid an unbounded walk",
                    self.limit
                );
            }
            let id = self.admit(what, &mut comment, parent, depth)?;
            out.push(comment);

            if !self.include_replies {
                continue;
            }
            let children = fetch_all_v2_results(
                self.client,
                what,
                &self.family.children_path(&id),
                &comment_query(),
            )
            .await?;
            pending.extend(
                children
                    .into_iter()
                    .rev()
                    .map(|child| (child, Some(id.clone()), depth + 1)),
            );
        }
        Ok(out)
    }

    /// Take one comment into the listing: read its id, refuse one already seen,
    /// and record what the traversal knows about it.
    ///
    /// `parentCommentId` is the collection the comment was read out of,
    /// `location` the path that was requested and `pageId` the page the listing
    /// was opened on, so none of them is a response field to trust. A root gets
    /// an explicit `null` parent, keeping "answers nothing" distinct from "not
    /// reported". `depth` counts from this listing's roots.
    fn admit(
        &mut self,
        what: &str,
        comment: &mut Value,
        parent: Option<String>,
        depth: usize,
    ) -> Result<String> {
        let Some(object) = comment.as_object_mut() else {
            anyhow::bail!("Failed to {what}: a comment was not a JSON object");
        };
        let Some(id) = object.get("id").and_then(Value::as_str).map(str::to_string) else {
            anyhow::bail!("Failed to {what}: a comment had no string 'id'");
        };
        if !self.seen.insert(id.clone()) {
            anyhow::bail!("Failed to {what}: comment {id} is reachable from itself");
        }

        // The response filter trims what the API sent; the four fields below
        // are the walk's own answers, and `location` is what makes an id this
        // module hands out a complete address. Filtering first is what keeps a
        // caller's `response_exclude_fields` from taking them — `get_comment`
        // stamps after filtering for the same reason.
        filter::apply(comment, self.client.config());
        let Some(object) = comment.as_object_mut() else {
            anyhow::bail!("Failed to {what}: a comment was not a JSON object");
        };

        object.insert("location".into(), json!(self.family.label()));
        object.insert("depth".into(), json!(depth));
        object.insert(
            "parentCommentId".into(),
            parent.map_or(Value::Null, Value::String),
        );
        if let Some(page_id) = self.page_id {
            object.insert("pageId".into(), json!(page_id));
        }
        Ok(id)
    }
}

/// Wrap a flattened comment listing in the standard envelope.
///
/// Unlike `v2_list_envelope` this does not filter: `ThreadWalk::admit` already
/// filtered each comment before stamping the fields the walk determined, and
/// filtering again here would remove exactly those.
fn comment_envelope(items: Vec<Value>, as_markdown: bool) -> Value {
    let mut envelope = json!({ "items": items });
    if as_markdown && let Some(comments) = envelope["items"].as_array_mut() {
        convert_comments_to_markdown(comments);
    }
    envelope
}

/// List a page's comments, replies included.
///
/// `families` selects which of the two the listing covers; `include_replies`
/// decides whether each root's thread is walked. The result is one flat,
/// depth-first array, so the `{"items": [...]}` envelope holds at any nesting
/// and every entry has the same shape — `parentCommentId`, `depth` and
/// `location` carry the thread structure through the flattening, and each entry
/// is a complete address for `get_comment` / `get_comment_replies`.
pub async fn get_comments(
    page_id: &str,
    families: &[CommentFamily],
    include_replies: bool,
    as_markdown: bool,
    client: &ApiClient,
) -> Result<Value> {
    let mut items = Vec::new();
    for &family in families {
        let roots = fetch_all_v2_results(
            client,
            "get comments",
            &family.page_path(page_id),
            &comment_query(),
        )
        .await?;
        items.extend(
            ThreadWalk::new(client, family, Some(page_id), include_replies)
                .collect("get comments", roots, None)
                .await?,
        );
    }
    Ok(comment_envelope(items, as_markdown))
}

/// List everything below one comment.
///
/// The requested comment is the origin rather than a member of the result, so
/// its direct replies are depth 0 — the roots of a listing are depth 0 either
/// way, which is what keeps this shape identical to `get_comments`'.
pub async fn get_comment_replies(
    comment_id: &str,
    family: CommentFamily,
    as_markdown: bool,
    client: &ApiClient,
) -> Result<Value> {
    let roots = fetch_all_v2_results(
        client,
        "get replies",
        &family.children_path(comment_id),
        &comment_query(),
    )
    .await?;
    let items = ThreadWalk::new(client, family, None, true)
        .collect("get replies", roots, Some(comment_id))
        .await?;
    Ok(comment_envelope(items, as_markdown))
}

/// Fetch one comment by id.
///
/// The family is part of the address, not something to discover: a 404 from the
/// wrong one is indistinguishable from a comment that was deleted or that the
/// caller cannot see, so retrying in the other family would collapse three
/// different answers into one. Every comment this module emits carries its
/// `location`, so an id it handed out is always a complete address.
pub async fn get_comment(
    comment_id: &str,
    family: CommentFamily,
    as_markdown: bool,
    client: &ApiClient,
) -> Result<Value> {
    let request = client
        .get(Service::Confluence, &family.single_path(comment_id))
        .await?
        .header("Accept", "application/json")
        .query(&[("body-format", "storage")]);
    let response = client.execute("get comment", request).await?;

    let mut data: Value = response.json().await?;
    filter::apply(&mut data, client.config());
    if as_markdown {
        convert_comments_to_markdown(std::slice::from_mut(&mut data));
    }
    if let Some(object) = data.as_object_mut() {
        object.insert("location".into(), json!(family.label()));
    }
    Ok(data)
}

/// Read the current `version.number` of a versioned v2 resource (page or
/// comment). v2 writes that bump a resource require the next version number, so
/// every updater reads the current one through this single helper instead of
/// re-implementing the GET. `include-version=true` is requested explicitly
/// because the page endpoint omits the version object otherwise; endpoints that
/// always include it ignore the redundant query param.
async fn fetch_version_number(client: &ApiClient, url: &str) -> Result<u64> {
    let request = client
        .get(Service::Confluence, url)
        .await?
        .header("Accept", "application/json")
        .query(&[("include-version", "true")]);
    let response = client.execute("fetch current version", request).await?;

    let data: Value = response.json().await?;
    data["version"]["number"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("Failed to read current version number"))
}

/// Fetch every page of a Confluence v2 list endpoint, following the
/// `_links.next` cursor until it is exhausted, and return the accumulated
/// `results`. v2 collections are cursor-paginated, so a single GET would
/// silently drop everything past the first page; routing every list through
/// here guarantees complete results.
///
/// `query` is applied to the first request only — each `next` link already
/// carries the original query (cursor, limit, `body-format`, …). v2 states
/// `next` from the site root (`/wiki/…`), so it is re-issued as a path; an
/// absolute link is reduced to its path first, never dialed as given.
async fn fetch_all_v2_results(
    client: &ApiClient,
    what: &str,
    path: &str,
    query: &[(&str, &str)],
) -> Result<Vec<Value>> {
    // Hard ceiling, matching `paginate`'s `MAX_PAGES` on the Jira side. The
    // `CursorTrail` below stops a cursor that repeats a page; a server handing
    // out a genuinely new cursor every time repeats nothing and stalls nothing,
    // so without this the walk would run until it ran out of memory.
    const MAX_CURSOR_PAGES: u32 = 10_000;
    let mut results: Vec<Value> = Vec::new();
    let mut next: Option<String> = None;
    let mut trail = CursorTrail::default();

    for _ in 0..MAX_CURSOR_PAGES {
        let request = match next.as_deref() {
            None => client.get(Service::Confluence, path).await?.query(query),
            Some(next_path) => client.get(Service::Confluence, next_path).await?,
        };

        let response = client
            .execute(what, request.header("Accept", "application/json"))
            .await?;

        // A v2 list page always carries a `results` array; its absence on a 2xx
        // means schema drift or a wrong-shaped response. Bail rather than
        // silently returning a short list — the same posture as the Jira-side
        // `paginate` helper.
        let data: Value = response.json().await?;
        let page = data["results"].as_array().ok_or_else(|| {
            anyhow::anyhow!("Failed to {}: response had no 'results' array", what)
        })?;
        results.extend(page.iter().cloned());

        // As above: absent or null is v2's end-of-list signal and nothing else
        // is. An empty string is not it — the v1 walk and the Jira token walk
        // both refuse one, and a completeness claim drawn from an undocumented
        // shape is the short list this walk exists to prevent.
        let candidate = &data["_links"]["next"];
        if candidate.is_null() {
            return Ok(results);
        }
        match candidate.as_str() {
            Some(link) => next = Some(trail.step(what, link)?),
            None => anyhow::bail!("Failed to {what}: '_links.next' was not a string: {candidate}"),
        }
    }

    anyhow::bail!(
        "Failed to {what}: exceeded {MAX_CURSOR_PAGES} pages without reaching the end of the \
         collection — aborting to avoid an unbounded walk"
    )
}

/// Build the standard `{"items": [...]}` envelope from a fully-paginated v2
/// list and apply the configured response filter. Every paginated list
/// endpoint funnels through here so the envelope and filtering stay identical.
fn v2_list_envelope(mut items: Vec<Value>, client: &ApiClient) -> Value {
    // The items, never the envelope: the wrapper is this CLI's contract rather
    // than the API's answer, so `response_exclude_fields` naming `items` would
    // otherwise return `{}` with exit 0 where a list was promised.
    for item in &mut items {
        filter::apply(item, client.config());
    }
    json!({ "items": items })
}

// --- Footer comments (write) ---------------------------------------------

/// Create a footer comment on a page. `parent_id` set → the comment is posted
/// as a reply to that comment (threaded); `None` → a top-level footer comment.
/// The body is storage-format HTML passed through verbatim — plain text is a
/// valid storage document, so no ADF-style conversion or content sniffing is
/// done (mirrors `create_page`/`update_page`).
pub async fn add_comment(
    page_id: &str,
    body: &str,
    parent_id: Option<&str>,
    client: &ApiClient,
) -> Result<Value> {
    // `pageId`/`parentCommentId` ride in the JSON body (serialized, not
    // interpolated into a path) so they need no path encoding. The v2
    // footer-comment API requires *exactly one* container id: a reply carries
    // only `parentCommentId` (the page is inferred from the parent comment), a
    // top-level comment only `pageId`. Sending both is rejected with a 400
    // ("Must specify one and only one of … pageId … or parentCommentId").
    let mut request_body = json!({
        "body": {
            "representation": "storage",
            "value": body,
        },
    });
    match parent_id {
        Some(parent) => request_body["parentCommentId"] = json!(parent),
        None => request_body["pageId"] = json!(page_id),
    }

    let request = client
        .post(Service::Confluence, "/wiki/api/v2/footer-comments")
        .await?
        .header("Content-Type", "application/json")
        .json(&request_body);
    let response = client.execute("add comment", request).await?;

    let data: Value = response.json().await?;
    Ok(json!({ "id": require_field(&data, "/id", "add comment")? }))
}

/// Update a footer comment's body. v2 requires the next version number, so the
/// current one is read first (same pattern as `update_page`).
pub async fn update_comment(comment_id: &str, body: &str, client: &ApiClient) -> Result<Value> {
    let url = format!(
        "/wiki/api/v2/footer-comments/{}",
        encode_path_segment(comment_id)
    );
    let next_version = fetch_version_number(client, &url).await? + 1;

    let request_body = json!({
        "version": { "number": next_version },
        "body": {
            "representation": "storage",
            "value": body,
        },
    });

    let request = client
        .put(Service::Confluence, &url)
        .await?
        .header("Content-Type", "application/json")
        .json(&request_body);
    let response = client.execute("update comment", request).await?;

    let data: Value = response.json().await?;
    Ok(json!({ "id": require_field(&data, "/id", "update comment")? }))
}

/// Delete a footer comment by id. The id is the specificity guard, so — like
/// the Jira `delete_comment`/`remove_link` family — no `--yes` is required.
pub async fn delete_comment(comment_id: &str, client: &ApiClient) -> Result<Value> {
    let url = format!(
        "/wiki/api/v2/footer-comments/{}",
        encode_path_segment(comment_id)
    );

    let request = client.delete(Service::Confluence, &url).await?;
    client.execute("delete comment", request).await?;

    Ok(json!({}))
}

fn extract_content_from_results(data: &mut Value, as_markdown: bool) -> Result<Vec<Value>> {
    // A present-but-empty `results` is a legitimate zero-match search; a
    // *missing* `results` on a 2xx is schema drift, so bail rather than report
    // an empty page (the same distinction `fetch_all_v2_results` enforces).
    let Some(results) = data.get_mut("results").and_then(|r| r.as_array_mut()) else {
        anyhow::bail!("search succeeded but its response had no 'results' array: {data}");
    };

    Ok(results
        .iter_mut()
        .map(|item| {
            // A result with no `content` is not a content hit — v1 search also
            // answers with space and user entities — and dropping it reported
            // no matches for a query that had them. It is returned as it came.
            let Some(content) = item.get_mut("content").filter(|c| !c.is_null()) else {
                return item.take();
            };
            let mut content = content.take();

            if as_markdown
                && let Some(html) = content
                    .get("body")
                    .and_then(|b| b.get("storage"))
                    .and_then(|s| s.get("value"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            {
                content["body"]["storage"]["value"] = Value::String(confluence_to_markdown(&html));
            }

            content
        })
        .collect())
}

fn convert_comments_to_markdown(comments: &mut [Value]) {
    for item in comments {
        let Some(body) = item
            .get_mut("body")
            .and_then(|b| b.get_mut("storage"))
            .and_then(|s| s.get_mut("value"))
        else {
            continue;
        };
        if let Some(html) = body.as_str().map(|s| s.to_string()) {
            *body = Value::String(confluence_to_markdown(&html));
        }
    }
}

#[cfg(test)]
#[path = "api/tests.rs"]
mod tests;
