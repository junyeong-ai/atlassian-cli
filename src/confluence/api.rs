use crate::client::{ApiClient, Service, extract_path_and_query};
use crate::config::Config;
use crate::confluence::fields::{apply_v2_filtering, build_search_expand};
use crate::filter;
use crate::http_utils::{content_type_for_filename, encode_path_segment};
use crate::markdown::confluence_to_markdown;
use crate::query_utils::{clause_detector, inject_filter};
use crate::response::{require_field, require_u64};
use anyhow::Result;
use regex::Regex;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::io::{self, Write};
use std::sync::LazyLock;
use std::time::Duration;
use tokio::time::sleep;

/// Operative page-size cap for the CQL search endpoint. The v1 search with
/// body expansion (which we always request) is throttled well below the
/// non-body ceiling, so 50 is the real maximum a single page returns.
const MAX_SEARCH_LIMIT: u32 = 50;

/// Detects an existing `space` scope so the configured space filter is not
/// injected on top of it. See `query_utils::clause_detector` for the operator
/// coverage (including the dotted `space.key`/`space.type` CQL forms) and
/// word-boundary rationale.
static SPACE_CLAUSE_RE: LazyLock<Regex> = LazyLock::new(|| clause_detector("space"));

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

    inject_filter(cql, &SPACE_CLAUSE_RE, &format!("space IN ({})", spaces))
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

    let items = extract_content_from_results(&mut data, as_markdown)?;
    let total = require_u64(&data, "/totalSize", "search")?;

    let mut output = json!({
        "items": items,
        "count": items.len(),
        "total": total,
    });

    filter::apply(&mut output, client.config());
    Ok(output)
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
    // that walk.
    const MAX_PAGES: u32 = 10_000;
    let mut all_items: Vec<Value> = Vec::new();
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
            // envelope's `total` is the exact `all_items.len()` (see below), so
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

        all_items.extend(items);

        eprintln!(
            "  Page {}: {} items (fetched: {}/{})",
            page_num,
            count,
            all_items.len(),
            total_size
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
        anyhow::bail!("search did not finish within {MAX_PAGES} pages");
    }

    eprintln!("\nTotal: {} items fetched", all_items.len());

    // See `jira::search_all` — Null signals `output_json` to skip, so the
    // trailing summary doesn't pollute the JSONL stream.
    if stream {
        Ok(Value::Null)
    } else {
        Ok(json!({
            "items": all_items,
            "total": all_items.len()
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

pub async fn get_page(
    page_id: &str,
    include_all_fields: Option<bool>,
    additional_includes: Option<Vec<String>>,
    as_markdown: bool,
    client: &ApiClient,
) -> Result<Value> {
    let url = format!("/wiki/api/v2/pages/{}", encode_path_segment(page_id));

    let query_params = apply_v2_filtering(include_all_fields, additional_includes);

    let request = client
        .get(Service::Confluence, &url)
        .await?
        .header("Accept", "application/json")
        .query(&query_params);
    let response = client.execute("get page", request).await?;

    let mut data: Value = response.json().await?;
    filter::apply(&mut data, client.config());

    if as_markdown {
        convert_page_to_markdown(&mut data);
    }

    Ok(data)
}

pub async fn get_page_children(page_id: &str, client: &ApiClient) -> Result<Value> {
    let path = format!(
        "/wiki/api/v2/pages/{}/children",
        encode_path_segment(page_id)
    );
    let items = fetch_all_v2_results(client, "get child pages", &path, &[]).await?;
    Ok(v2_list_envelope(items, client))
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
fn comment_envelope(items: Vec<Value>, as_markdown: bool, client: &ApiClient) -> Value {
    let mut envelope = v2_list_envelope(items, client);
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
    Ok(comment_envelope(items, as_markdown, client))
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
    Ok(comment_envelope(items, as_markdown, client))
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

pub async fn create_page(
    space_key: &str,
    title: &str,
    content: &str,
    parent_id: Option<&str>,
    include_all_fields: Option<bool>,
    additional_includes: Option<Vec<String>>,
    client: &ApiClient,
) -> Result<Value> {
    // Resolve the space key to its numeric id via the shared helper (also used
    // by the `space` discovery commands).
    let space_id = resolve_space_id(space_key, client).await?;

    // Now create the page with v2 API
    let url = "/wiki/api/v2/pages";

    let query_params = apply_v2_filtering(include_all_fields, additional_includes);

    let mut body = json!({
        "spaceId": space_id,
        "title": title,
        "body": {
            "representation": "storage",
            "value": content
        }
    });
    // `parentId` nests the new page under an existing page; omitting it creates
    // the page at the space root.
    if let Some(parent) = parent_id {
        body["parentId"] = json!(parent);
    }

    let request = client
        .post(Service::Confluence, url)
        .await?
        .header("Content-Type", "application/json")
        .query(&query_params)
        .json(&body);
    let response = client.execute("create page", request).await?;

    let data: Value = response.json().await?;
    Ok(json!({
        "id": require_field(&data, "/id", "create page")?,
        "title": data["title"],
    }))
}

pub async fn update_page(
    page_id: &str,
    title: &str,
    content: &str,
    parent_id: Option<&str>,
    include_all_fields: Option<bool>,
    additional_includes: Option<Vec<String>>,
    client: &ApiClient,
) -> Result<Value> {
    let url = format!("/wiki/api/v2/pages/{}", encode_path_segment(page_id));
    let next_version = fetch_version_number(client, &url).await? + 1;

    let query_params = apply_v2_filtering(include_all_fields, additional_includes);

    // `status: "current"` is part of the v2 update contract and keeps the page
    // published; this CLI only edits live pages, so it is always "current".
    let mut body = json!({
        "id": page_id,
        "status": "current",
        "title": title,
        "body": {
            "representation": "storage",
            "value": content
        },
        "version": {
            "number": next_version
        }
    });
    // `parentId` re-parents the page when supplied. The v2 PUT preserves the
    // page's existing parent when the field is omitted, so a plain update never
    // detaches a child page.
    if let Some(parent) = parent_id {
        body["parentId"] = json!(parent);
    }

    let request = client
        .put(Service::Confluence, &url)
        .await?
        .header("Content-Type", "application/json")
        .query(&query_params)
        .json(&body);
    let response = client.execute("update page", request).await?;

    let data: Value = response.json().await?;
    Ok(json!({
        "id": require_field(&data, "/id", "update page")?,
        "version": require_u64(&data, "/version/number", "update page")?,
    }))
}

/// Move a page to the Confluence trash (v2 `DELETE` is recoverable, unlike
/// Jira issue deletion). Still a whole-resource destruction, so the CLI layer
/// requires an explicit `--yes`.
pub async fn delete_page(page_id: &str, client: &ApiClient) -> Result<Value> {
    let url = format!("/wiki/api/v2/pages/{}", encode_path_segment(page_id));

    let request = client.delete(Service::Confluence, &url).await?;
    client.execute("delete page", request).await?;

    Ok(json!({}))
}

/// Fetch a single space object by key via the v2 spaces endpoint (`?keys=`),
/// or `None` when no space matches. Returns the raw API object **without**
/// `filter::apply` so each caller decides: `resolve_space_id` reads the
/// unfiltered `id`; `get_space` applies the field filter before handing the
/// object to the user.
async fn fetch_space_by_key(space_key: &str, client: &ApiClient) -> Result<Option<Value>> {
    let request = client
        .get(Service::Confluence, "/wiki/api/v2/spaces")
        .await?
        .header("Accept", "application/json")
        .query(&[("keys", space_key)]);
    let response = client
        .execute(&format!("get space '{space_key}'"), request)
        .await?;

    let data: Value = response.json().await?;
    // No `results` array on a 2xx is drift, not an answer: reading it as "not
    // there" makes a lookup report an absence it never established.
    let Some(results) = data["results"].as_array() else {
        anyhow::bail!("lookup succeeded but its response had no 'results' array: {data}");
    };
    Ok(results.first().cloned())
}

/// Resolve a Confluence space key to its numeric space id. The single
/// space-key→id lookup, shared by `create_page` and the `space` commands.
pub async fn resolve_space_id(space_key: &str, client: &ApiClient) -> Result<String> {
    // Two facts, kept apart: no space by that key, and a space whose `id` the
    // response did not carry. Collapsing them reports an absence never
    // established — the property lookup below already keeps them separate.
    let Some(space) = fetch_space_by_key(space_key, client).await? else {
        anyhow::bail!("Space '{space_key}' not found");
    };
    space["id"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("Space '{space_key}' has no string 'id': {space}"))
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
        // `paginate_values` helper.
        let data: Value = response.json().await?;
        let page = data["results"].as_array().ok_or_else(|| {
            anyhow::anyhow!("Failed to {}: response had no 'results' array", what)
        })?;
        results.extend(page.iter().cloned());

        // As above: absent, null or empty ends the collection; a `next` of any
        // other shape is drift rather than an answer.
        let candidate = &data["_links"]["next"];
        if candidate.is_null() {
            return Ok(results);
        }
        match candidate.as_str() {
            Some("") => return Ok(results),
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
fn v2_list_envelope(items: Vec<Value>, client: &ApiClient) -> Value {
    let mut envelope = json!({ "items": items });
    filter::apply(&mut envelope, client.config());
    envelope
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

// --- Labels ---------------------------------------------------------------

/// List the labels on a page (v2).
pub async fn get_labels(page_id: &str, client: &ApiClient) -> Result<Value> {
    let path = format!("/wiki/api/v2/pages/{}/labels", encode_path_segment(page_id));
    let items = fetch_all_v2_results(client, "get labels", &path, &[]).await?;
    Ok(v2_list_envelope(items, client))
}

/// Add a label to a page. v2 exposes no label-write endpoint, so this uses the
/// stable v1 content-label API. v1 POST adds without clearing existing labels,
/// so repeated calls are safe for agent retries. Side-effect only → `{}`.
pub async fn add_label(page_id: &str, label: &str, client: &ApiClient) -> Result<Value> {
    let url = format!(
        "/wiki/rest/api/content/{}/label",
        encode_path_segment(page_id)
    );

    let request_body = json!([{ "prefix": "global", "name": label }]);

    let request = client
        .post(Service::Confluence, &url)
        .await?
        .header("Content-Type", "application/json")
        .json(&request_body);
    client.execute("add label", request).await?;

    Ok(json!({}))
}

/// Remove a label from a page via the v1 content-label API. The label name is
/// the specificity guard (a targeted sub-resource removal), so no `--yes`. The
/// name rides in a query param via reqwest's builder, never the path.
pub async fn remove_label(page_id: &str, label: &str, client: &ApiClient) -> Result<Value> {
    let url = format!(
        "/wiki/rest/api/content/{}/label",
        encode_path_segment(page_id)
    );

    let request = client
        .delete(Service::Confluence, &url)
        .await?
        .query(&[("name", label)]);
    client.execute("remove label", request).await?;

    Ok(json!({}))
}

// --- Content properties (structured JSON metadata) ------------------------

/// List all content properties on a page (v2). Properties are arbitrary JSON
/// key/value metadata attached to a page — a clean place to store structured,
/// machine-read state alongside the human-authored body.
pub async fn get_properties(page_id: &str, client: &ApiClient) -> Result<Value> {
    let path = format!(
        "/wiki/api/v2/pages/{}/properties",
        encode_path_segment(page_id)
    );
    let items = fetch_all_v2_results(client, "get properties", &path, &[]).await?;
    Ok(v2_list_envelope(items, client))
}

/// Create or update a content property on a page (upsert keyed by `key`). When
/// a property with `key` already exists it is updated with the required version
/// bump read from the same lookup; otherwise a new one is created. `value` is
/// arbitrary JSON. Returns `{"id": ...}`.
pub async fn set_property(
    page_id: &str,
    key: &str,
    value: Value,
    client: &ApiClient,
) -> Result<Value> {
    let encoded_page = encode_path_segment(page_id);
    let collection_url = format!("/wiki/api/v2/pages/{}/properties", encoded_page);

    let existing = fetch_property_by_key(client, &collection_url, key).await?;

    let request = if let Some(prop) = existing {
        let prop_id = prop["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Property lookup returned no id"))?;
        let next_version = prop["version"]["number"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Failed to read current property version"))?
            + 1;
        let url = format!(
            "/wiki/api/v2/pages/{}/properties/{}",
            encoded_page,
            encode_path_segment(prop_id)
        );
        let request_body = json!({
            "key": key,
            "value": value,
            "version": { "number": next_version },
        });
        client
            .put(Service::Confluence, &url)
            .await?
            .header("Content-Type", "application/json")
            .json(&request_body)
    } else {
        let request_body = json!({ "key": key, "value": value });
        client
            .post(Service::Confluence, &collection_url)
            .await?
            .header("Content-Type", "application/json")
            .json(&request_body)
    };

    let response = client.execute("set property", request).await?;

    let data: Value = response.json().await?;
    Ok(json!({ "id": require_field(&data, "/id", "set property")? }))
}

/// Delete a content property from a page by key. The key is the specificity
/// guard → no `--yes`. A missing key is reported as an error rather than a
/// silent success.
pub async fn delete_property(page_id: &str, key: &str, client: &ApiClient) -> Result<Value> {
    let encoded_page = encode_path_segment(page_id);
    let collection_url = format!("/wiki/api/v2/pages/{}/properties", encoded_page);

    let prop = fetch_property_by_key(client, &collection_url, key)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Property '{}' not found", key))?;
    let prop_id = prop["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Property lookup returned no id"))?;

    let url = format!(
        "/wiki/api/v2/pages/{}/properties/{}",
        encoded_page,
        encode_path_segment(prop_id)
    );

    let request = client.delete(Service::Confluence, &url).await?;
    client.execute("delete property", request).await?;

    Ok(json!({}))
}

/// Look up a single content property by key on a page's property collection.
/// Returns the property object, or `None` when no property with that key
/// exists. Shared by `set_property` (create-vs-update decision) and
/// `delete_property` (id resolution); mirrors `fetch_space_by_key`.
async fn fetch_property_by_key(
    client: &ApiClient,
    collection_url: &str,
    key: &str,
) -> Result<Option<Value>> {
    let request = client
        .get(Service::Confluence, collection_url)
        .await?
        .header("Accept", "application/json")
        .query(&[("key", key)]);
    let response = client.execute("look up property", request).await?;

    let data: Value = response.json().await?;
    // No `results` array on a 2xx is drift, not an answer: reading it as "not
    // there" makes a lookup report an absence it never established.
    let Some(results) = data["results"].as_array() else {
        anyhow::bail!("lookup succeeded but its response had no 'results' array: {data}");
    };
    Ok(results.first().cloned())
}

// --- Spaces ---------------------------------------------------------------

/// List spaces visible to the caller (v2), following pagination to completion.
pub async fn get_spaces(client: &ApiClient) -> Result<Value> {
    let items = fetch_all_v2_results(client, "list spaces", "/wiki/api/v2/spaces", &[]).await?;
    Ok(v2_list_envelope(items, client))
}

/// Fetch a single space by key (v2). Returns the space object after filtering.
pub async fn get_space(space_key: &str, client: &ApiClient) -> Result<Value> {
    let mut space = fetch_space_by_key(space_key, client)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Space '{}' not found", space_key))?;
    filter::apply(&mut space, client.config());
    Ok(space)
}

// --- Attachments ----------------------------------------------------------

/// List the attachments on a page (v2).
pub async fn get_attachments(page_id: &str, client: &ApiClient) -> Result<Value> {
    let path = format!(
        "/wiki/api/v2/pages/{}/attachments",
        encode_path_segment(page_id)
    );
    let items = fetch_all_v2_results(client, "get attachments", &path, &[]).await?;
    Ok(v2_list_envelope(items, client))
}

/// Upload a local file as an attachment on a page. v2 exposes no
/// attachment-create endpoint, so this uses the stable v1 multipart API (the
/// same v1 exception as label writes). `PUT` upserts by filename — a new file
/// is created, an existing one gets a new version — so repeated calls are safe
/// for agent retries. The `X-Atlassian-Token: nocheck` header is required by
/// Confluence to bypass its XSRF check on multipart uploads.
///
/// Under OAuth this needs the `write:attachment:confluence` scope (basic-auth
/// tokens carry the user's own permissions and are unaffected).
pub async fn upload_attachment(
    page_id: &str,
    file_path: &str,
    comment: Option<&str>,
    minor_edit: bool,
    content_type: Option<&str>,
    client: &ApiClient,
) -> Result<Value> {
    let bytes = std::fs::read(file_path)
        .map_err(|e| anyhow::anyhow!("Failed to read '{}': {}", file_path, e))?;
    // The displayed attachment name is the path's final component; this is
    // deterministic path parsing, not content sniffing.
    let file_name = std::path::Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("Could not derive a file name from '{}'", file_path))?
        .to_string();

    // An explicit `--content-type` wins; otherwise the type is mapped from the
    // extension so images/PDFs render inline instead of becoming opaque
    // `application/octet-stream` downloads.
    let mime = content_type.unwrap_or_else(|| content_type_for_filename(&file_name));

    // `minorEdit` is always sent (the v1 endpoint expects it); `true` suppresses
    // the watcher notification that a re-upload would otherwise fire.
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(file_name)
        .mime_str(mime)
        .map_err(|e| anyhow::anyhow!("Invalid content type '{}': {}", mime, e))?;
    let mut form = reqwest::multipart::Form::new()
        .part("file", part)
        .text("minorEdit", minor_edit.to_string());
    if let Some(c) = comment {
        form = form.text("comment", c.to_string());
    }

    let url = format!(
        "/wiki/rest/api/content/{}/child/attachment",
        encode_path_segment(page_id)
    );

    let request = client
        .put(Service::Confluence, &url)
        .await?
        .header("X-Atlassian-Token", "nocheck")
        .multipart(form);
    let response = client.execute("upload attachment", request).await?;

    // v1 wraps the created/updated attachment in a `results` array.
    let data: Value = response.json().await?;
    Ok(json!({ "id": require_field(&data, "/results/0/id", "upload attachment")? }))
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

fn convert_page_to_markdown(data: &mut Value) {
    let Some(body) = data
        .get_mut("body")
        .and_then(|b| b.get_mut("storage"))
        .and_then(|s| s.get_mut("value"))
    else {
        return;
    };
    if let Some(html) = body.as_str().map(|s| s.to_string()) {
        *body = Value::String(confluence_to_markdown(&html));
    }
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
mod tests {
    use super::*;
    use crate::test_utils::{create_test_config_with_filters, mock_client};
    use wiremock::matchers::{
        body_json, body_string_contains, header, method, path, query_param, query_param_is_missing,
    };
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn create_test_config(confluence_spaces_filter: Vec<String>) -> Config {
        create_test_config_with_filters(vec![], confluence_spaces_filter)
    }

    #[test]
    fn effective_search_limit_clamps_to_cap() {
        assert_eq!(effective_search_limit(10), 10);
        assert_eq!(effective_search_limit(1000), MAX_SEARCH_LIMIT);
        assert_eq!(effective_search_limit(MAX_SEARCH_LIMIT), MAX_SEARCH_LIMIT);
        assert_eq!(effective_search_limit(0), 1);
    }

    #[tokio::test]
    async fn integ_search_all_honors_limit_on_first_page() {
        let server = MockServer::start().await;
        // The `--all` first page must carry the user's clamped limit, not the
        // hardcoded body cap. limit=10 → query param "10".
        Mock::given(method("GET"))
            .and(path("/wiki/rest/api/search"))
            .and(query_param("limit", "10"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [],
                "totalSize": 0
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = search_all("type = page", 10, None, None, false, false, &client)
            .await
            .unwrap();
        assert_eq!(result["total"], 0);
    }

    #[tokio::test]
    async fn integ_search_returns_envelope_with_server_total() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wiki/rest/api/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "content": { "id": "1", "title": "P" } }],
                "totalSize": 7
            })))
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = search("type = page", 10, None, None, false, &client)
            .await
            .unwrap();
        assert_eq!(result["total"], 7);
        assert_eq!(result["count"], 1);
        assert_eq!(result["items"][0]["id"], "1");
    }

    #[tokio::test]
    async fn integ_search_bails_on_missing_results() {
        // A 2xx that omits `results` is schema drift, not a zero-match search.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wiki/rest/api/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "totalSize": 0 })))
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let err = search("type = page", 10, None, None, false, &client)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("no 'results' array"), "got: {err}");
    }

    /// A lookup's 2xx without a `results` array is drift; reading it as "not
    /// found" reports an absence nothing established.
    #[tokio::test]
    async fn integ_space_lookup_bails_on_missing_results() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/spaces"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "_links": {} })))
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let err = resolve_space_id("ENG", &client)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("no 'results' array"), "got: {err}");
    }

    /// A space that came back without an `id` is not a space that is not there.
    #[tokio::test]
    async fn integ_a_space_without_an_id_is_not_a_missing_space() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/spaces"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "results": [{ "key": "ENG" }] })),
            )
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let err = resolve_space_id("ENG", &client)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("no string 'id'"), "got: {err}");
        assert!(!err.contains("not found"), "got: {err}");
    }

    /// v1 search answers space and user entities without a `content` object.
    /// Dropping them reported no matches for a query that had them, and the
    /// count it left behind then ended the `--all` crawl.
    #[tokio::test]
    async fn integ_search_keeps_a_result_that_carries_no_content() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wiki/rest/api/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "space": { "key": "ENG" }, "title": "Engineering" }],
                "totalSize": 1
            })))
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = search("type = space", 10, None, None, false, &client)
            .await
            .unwrap();

        assert_eq!(result["count"], 1, "{result}");
        assert_eq!(result["items"][0]["title"], "Engineering", "{result}");
    }

    #[tokio::test]
    async fn integ_search_requires_numeric_total_size() {
        // Present `results` but absent `totalSize` must fail, not fabricate one.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wiki/rest/api/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let err = search("type = page", 10, None, None, false, &client)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("totalSize"), "got: {err}");
    }

    #[tokio::test]
    async fn integ_get_page_encodes_page_id() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/pages/12%20345"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "12 345" })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = get_page("12 345", None, None, false, &client)
            .await
            .unwrap();
        assert_eq!(result["id"], "12 345");
    }

    #[tokio::test]
    async fn integ_delete_page_encodes_id() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/wiki/api/v2/pages/9%2F9"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = delete_page("9/9", &client).await.unwrap();
        assert_eq!(result, json!({}));
    }

    #[test]
    fn test_rate_limit_delay_default() {
        let config = create_test_config(vec![]);
        assert_eq!(config.performance.rate_limit_delay_ms, 200);
    }

    #[test]
    fn test_apply_space_filter_injection() {
        let config = create_test_config(vec!["SPACE1".to_string(), "SPACE2".to_string()]);
        let result = apply_space_filter("type = page", &config);
        assert_eq!(result, "space IN (\"SPACE1\",\"SPACE2\") AND (type = page)");
    }

    #[test]
    fn test_apply_space_filter_not_injected_when_present() {
        let config = create_test_config(vec!["SPACE1".to_string()]);
        let result = apply_space_filter("space = MYSPACE AND type = page", &config);
        assert_eq!(result, "space = MYSPACE AND type = page");
    }

    #[test]
    fn test_apply_space_filter_empty_filter() {
        let config = create_test_config(vec![]);
        let result = apply_space_filter("type = page", &config);
        assert_eq!(result, "type = page");
    }

    #[test]
    fn test_apply_space_filter_ignores_quoted_keyword() {
        let config = create_test_config(vec!["SPACE1".to_string()]);
        // The substring `space =` inside a quoted title must NOT suppress
        // the filter injection — the regex runs against a masked CQL string.
        let result = apply_space_filter("title ~ \"space = anywhere\"", &config);
        assert!(
            result.starts_with("space IN (\"SPACE1\")"),
            "filter should be injected, got: {result}"
        );
    }

    #[test]
    fn test_apply_space_filter_skips_word_boundary_non_match() {
        let config = create_test_config(vec!["SPACE1".to_string()]);
        // `mySpace = X` is not a `space` clause — the word boundary regex
        // must not treat it as one.
        let result = apply_space_filter("mySpace = X", &config);
        assert_eq!(result, "space IN (\"SPACE1\") AND (mySpace = X)");
    }

    #[test]
    fn test_apply_space_filter_whitespace_only_cql_collapses_to_bare_filter() {
        let config = create_test_config(vec!["SPACE1".to_string()]);
        // Whitespace-only CQL collapses to a bare filter — no dangling
        // `AND (   )`. Matches the Jira-side behavior in apply_project_filter.
        let result = apply_space_filter("   ", &config);
        assert_eq!(result, "space IN (\"SPACE1\")");
    }

    /// v1 pagination resolved end to end: join `_links`, then reduce.
    fn v1_resolved(base: Value, next: &str) -> Option<String> {
        link_path(&v1_next_link(&base, next)).map(str::to_owned)
    }

    #[test]
    fn v1_joins_a_rooted_next_onto_the_base_path() {
        // `_links.base` carries the `/wiki` prefix that `next` omits.
        assert_eq!(
            v1_resolved(
                json!({ "base": "https://test.atlassian.net/wiki" }),
                "/rest/api/search?cql=type%3Dpage&cursor=abc123"
            )
            .unwrap(),
            "/wiki/rest/api/search?cql=type%3Dpage&cursor=abc123"
        );
    }

    #[test]
    fn v1_leaves_an_absolute_next_unjoined() {
        // An absolute `next` already carries `/wiki`; joining would double it.
        assert_eq!(
            v1_resolved(
                json!({ "base": "https://test.atlassian.net/wiki" }),
                "https://test.atlassian.net/wiki/rest/api/search?cursor=xyz"
            )
            .unwrap(),
            "/wiki/rest/api/search?cursor=xyz"
        );
    }

    #[test]
    fn v1_survives_a_missing_base() {
        assert_eq!(
            v1_resolved(json!({}), "/rest/api/search?cursor=z").unwrap(),
            "/rest/api/search?cursor=z"
        );
    }

    #[test]
    fn link_path_discards_the_host_a_response_names() {
        // The decisive property: a pagination link pointing at a foreign host
        // contributes its path only. The host always comes from `build_url`,
        // so credentials cannot be steered elsewhere by a response body.
        assert_eq!(
            link_path("https://evil.example/wiki/api/v2/pages?cursor=x"),
            Some("/wiki/api/v2/pages?cursor=x")
        );
        assert_eq!(
            v1_resolved(
                json!({ "base": "https://test.atlassian.net/wiki" }),
                "https://evil.example/wiki/rest/api/search?cursor=x"
            )
            .unwrap(),
            "/wiki/rest/api/search?cursor=x"
        );
        // A `base` naming a foreign host is reduced the same way.
        assert_eq!(
            v1_resolved(
                json!({ "base": "https://evil.example/wiki" }),
                "/rest/api/search"
            )
            .unwrap(),
            "/wiki/rest/api/search"
        );
    }

    #[test]
    fn link_path_passes_rooted_links_through_and_rejects_hostless_absolutes() {
        assert_eq!(
            link_path("/wiki/api/v2/pages?cursor=x"),
            Some("/wiki/api/v2/pages?cursor=x")
        );
        assert_eq!(link_path("https://test.atlassian.net"), None);
    }

    #[test]
    fn link_path_rejects_a_link_that_would_extend_the_authority() {
        // No scheme and no leading `/`. Treating it as a path would build
        // `https://site@evil.example/…`, which RFC 3986 resolves with
        // `evil.example` as the host and `site` as userinfo — the request's
        // credentials would go to a host named by the response.
        for link in [
            "@evil.example/wiki/api/v2/pages?cursor=x",
            "evil.example/wiki/api/v2/pages",
            ":@evil.example/x",
        ] {
            assert_eq!(link_path(link), None, "must reject {link}");
            assert_eq!(
                v1_resolved(json!({ "base": "https://s.atlassian.net/wiki" }), link),
                None,
                "must reject {link}"
            );
        }
    }

    #[test]
    fn link_path_keeps_a_rooted_link_whose_query_contains_a_scheme() {
        // Confluence echoes `cql` into `next` without percent-encoding it, so
        // searching for a URL produces a genuinely rooted link containing
        // `://`. Reading that as "absolute" would drop the path and cursor.
        let n = "/wiki/api/v2/pages?cursor=abc&cql=text~http://x.example/p";
        assert_eq!(link_path(n), Some(n));

        let v1 = "/rest/api/search?cql=text~%22http://x.example%22&cursor=abc";
        assert_eq!(
            v1_resolved(json!({ "base": "https://s.atlassian.net/wiki" }), v1).unwrap(),
            format!("/wiki{v1}")
        );
    }

    #[test]
    fn cursor_trail_rejects_a_revisited_path() {
        let mut trail = CursorTrail::default();
        assert_eq!(trail.step("search", "/a?cursor=1").unwrap(), "/a?cursor=1");
        assert_eq!(trail.step("search", "/a?cursor=2").unwrap(), "/a?cursor=2");
        // Same resolved path behind a different host: still not advancing.
        let err = trail
            .step("search", "https://elsewhere.invalid/a?cursor=1")
            .unwrap_err()
            .to_string();
        assert!(err.contains("did not advance"), "{err}");
    }

    #[test]
    fn cursor_trail_rejects_a_link_with_no_path() {
        let err = CursorTrail::default()
            .step("search", "https://elsewhere.invalid")
            .unwrap_err()
            .to_string();
        assert!(err.contains("no path"), "{err}");
    }

    #[test]
    fn every_built_url_keeps_the_configured_host_as_the_authority() {
        // Parse rather than string-compare: the defect this guards against is
        // precisely one that string inspection reads as harmless.
        use crate::auth::AuthStrategy;
        let basic =
            crate::auth::BasicStrategy::new(Some("test.atlassian.net"), "u@x".into(), "tk".into())
                .unwrap();
        for path in [
            "/wiki/api/v2/pages",
            "@evil.example/wiki",
            "//evil.example/wiki",
            "wiki/api/v2/pages",
        ] {
            let url = reqwest::Url::parse(&basic.build_url(Service::Confluence, path)).unwrap();
            assert_eq!(url.host_str(), Some("test.atlassian.net"), "path {path}");
            assert_eq!(url.username(), "", "path {path} leaked userinfo");
        }
    }

    #[tokio::test]
    async fn integ_create_page_resolves_space_then_posts_storage_body() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/spaces"))
            .and(query_param("keys", "ENG"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "id": "sid" }]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/wiki/api/v2/pages"))
            .and(body_json(json!({
                "spaceId": "sid",
                "title": "Spec",
                "body": { "representation": "storage", "value": "<p>x</p>" }
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "id": "pid", "title": "Spec"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = create_page("ENG", "Spec", "<p>x</p>", None, None, None, &client)
            .await
            .unwrap();
        assert_eq!(result, json!({ "id": "pid", "title": "Spec" }));
    }

    #[tokio::test]
    async fn integ_create_page_nests_under_parent() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/spaces"))
            .and(query_param("keys", "ENG"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "results": [{ "id": "sid" }] })),
            )
            .mount(&server)
            .await;
        // `--parent` rides in the body as `parentId`; body_json is exact, so its
        // presence (and absence in the root-level test above) is asserted.
        Mock::given(method("POST"))
            .and(path("/wiki/api/v2/pages"))
            .and(body_json(json!({
                "spaceId": "sid",
                "title": "Child",
                "body": { "representation": "storage", "value": "<p>x</p>" },
                "parentId": "999"
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "id": "pid", "title": "Child"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = create_page("ENG", "Child", "<p>x</p>", Some("999"), None, None, &client)
            .await
            .unwrap();
        assert_eq!(result["id"], "pid");
    }

    #[tokio::test]
    async fn integ_update_page_reads_version_then_puts() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/pages/12345"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "version": { "number": 5 } })),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/wiki/api/v2/pages/12345"))
            .and(body_json(json!({
                "id": "12345",
                "status": "current",
                "title": "Updated",
                "body": { "representation": "storage", "value": "<p>y</p>" },
                "version": { "number": 6 }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "12345", "version": { "number": 6 }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = update_page("12345", "Updated", "<p>y</p>", None, None, None, &client)
            .await
            .unwrap();
        assert_eq!(result, json!({ "id": "12345", "version": 6 }));
    }

    #[tokio::test]
    async fn integ_update_page_reparents_when_parent_given() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/pages/12345"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "version": { "number": 5 } })),
            )
            .mount(&server)
            .await;
        // `--parent` adds `parentId` to the PUT body; body_json is exact, so the
        // sibling test above (no parentId) and this one together pin both paths.
        Mock::given(method("PUT"))
            .and(path("/wiki/api/v2/pages/12345"))
            .and(body_json(json!({
                "id": "12345",
                "status": "current",
                "title": "Updated",
                "body": { "representation": "storage", "value": "<p>y</p>" },
                "version": { "number": 6 },
                "parentId": "777"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "12345", "version": { "number": 6 }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = update_page(
            "12345",
            "Updated",
            "<p>y</p>",
            Some("777"),
            None,
            None,
            &client,
        )
        .await
        .unwrap();
        assert_eq!(result["version"], 6);
    }

    // --- Footer comment write -------------------------------------------

    #[tokio::test]
    async fn integ_add_comment_posts_storage_body_and_returns_id() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/wiki/api/v2/footer-comments"))
            .and(body_json(json!({
                "pageId": "123",
                "body": { "representation": "storage", "value": "<p>hi</p>" }
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "id": "555" })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = add_comment("123", "<p>hi</p>", None, &client)
            .await
            .unwrap();
        assert_eq!(result, json!({ "id": "555" }));
    }

    #[tokio::test]
    async fn integ_add_comment_reply_includes_parent() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/wiki/api/v2/footer-comments"))
            // A reply carries ONLY parentCommentId — sending pageId too makes
            // the real v2 API 400 ("one and only one of … pageId … or
            // parentCommentId"). body_json is an exact match, so the absence of
            // pageId here is asserted.
            .and(body_json(json!({
                "body": { "representation": "storage", "value": "ok" },
                "parentCommentId": "999"
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "id": "556" })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = add_comment("123", "ok", Some("999"), &client)
            .await
            .unwrap();
        assert_eq!(result["id"], "556");
    }

    #[tokio::test]
    async fn integ_update_comment_reads_version_then_puts() {
        let server = MockServer::start().await;
        // First the current version is read, then the PUT bumps it to +1.
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/footer-comments/77"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "id": "77", "version": { "number": 3 } })),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/wiki/api/v2/footer-comments/77"))
            .and(body_json(json!({
                "version": { "number": 4 },
                "body": { "representation": "storage", "value": "edited" }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "77" })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = update_comment("77", "edited", &client).await.unwrap();
        assert_eq!(result["id"], "77");
    }

    #[tokio::test]
    async fn integ_delete_comment_encodes_id() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/wiki/api/v2/footer-comments/7%2F7"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = delete_comment("7/7", &client).await.unwrap();
        assert_eq!(result, json!({}));
    }

    // --- Labels ----------------------------------------------------------

    #[tokio::test]
    async fn integ_add_label_posts_v1_global_prefix() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/wiki/rest/api/content/123/label"))
            .and(body_json(json!([{ "prefix": "global", "name": "urgent" }])))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = add_label("123", "urgent", &client).await.unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn integ_remove_label_passes_name_query() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/wiki/rest/api/content/123/label"))
            .and(query_param("name", "urgent"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = remove_label("123", "urgent", &client).await.unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn integ_get_labels_returns_items_envelope() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/pages/123/labels"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "id": "1", "name": "urgent" }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = get_labels("123", &client).await.unwrap();
        assert_eq!(result["items"][0]["name"], "urgent");
    }

    // --- Content properties ---------------------------------------------

    #[tokio::test]
    async fn integ_get_properties_returns_items_envelope() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/pages/123/properties"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "id": "p1", "key": "state", "value": { "phase": 1 } }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = get_properties("123", &client).await.unwrap();
        assert_eq!(result["items"][0]["key"], "state");
    }

    #[tokio::test]
    async fn integ_set_property_creates_when_absent() {
        let server = MockServer::start().await;
        // Lookup by key returns no match → POST a new property.
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/pages/123/properties"))
            .and(query_param("key", "state"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/wiki/api/v2/pages/123/properties"))
            .and(body_json(
                json!({ "key": "state", "value": { "phase": 2 } }),
            ))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "id": "p9" })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = set_property("123", "state", json!({ "phase": 2 }), &client)
            .await
            .unwrap();
        assert_eq!(result, json!({ "id": "p9" }));
    }

    #[tokio::test]
    async fn integ_set_property_updates_when_present() {
        let server = MockServer::start().await;
        // Lookup returns an existing property with version 4 → PUT with 5.
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/pages/123/properties"))
            .and(query_param("key", "state"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "id": "p1", "key": "state", "version": { "number": 4 } }]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/wiki/api/v2/pages/123/properties/p1"))
            .and(body_json(json!({
                "key": "state",
                "value": { "phase": 3 },
                "version": { "number": 5 }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "p1" })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = set_property("123", "state", json!({ "phase": 3 }), &client)
            .await
            .unwrap();
        assert_eq!(result, json!({ "id": "p1" }));
    }

    #[tokio::test]
    async fn integ_delete_property_resolves_key_then_deletes() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/pages/123/properties"))
            .and(query_param("key", "state"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "id": "p1", "key": "state" }]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/wiki/api/v2/pages/123/properties/p1"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = delete_property("123", "state", &client).await.unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn integ_delete_property_missing_key_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/pages/123/properties"))
            .and(query_param("key", "ghost"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let err = delete_property("123", "ghost", &client)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("not found"), "got: {err}");
    }

    // --- Spaces ----------------------------------------------------------

    #[tokio::test]
    async fn integ_get_spaces_returns_items_envelope() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/spaces"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "id": "1", "key": "ENG" }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = get_spaces(&client).await.unwrap();
        assert_eq!(result["items"][0]["key"], "ENG");
    }

    #[tokio::test]
    async fn integ_get_space_filters_by_key_and_returns_single() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/spaces"))
            .and(query_param("keys", "ENG"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "id": "1", "key": "ENG" }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = get_space("ENG", &client).await.unwrap();
        assert_eq!(result["id"], "1");
        assert_eq!(result["key"], "ENG");
    }

    #[tokio::test]
    async fn integ_resolve_space_id_extracts_numeric_id() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/spaces"))
            .and(query_param("keys", "ENG"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "id": "42", "key": "ENG" }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let id = resolve_space_id("ENG", &client).await.unwrap();
        assert_eq!(id, "42");
    }

    // --- Attachments -----------------------------------------------------

    #[tokio::test]
    async fn integ_get_attachments_returns_items_envelope() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/pages/123/attachments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "id": "att1", "title": "spec.pdf" }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = get_attachments("123", &client).await.unwrap();
        assert_eq!(result["items"][0]["title"], "spec.pdf");
    }

    #[tokio::test]
    async fn integ_upload_attachment_puts_multipart_with_token_header() {
        let server = MockServer::start().await;
        // The v1 upload is a multipart PUT guarded by the XSRF-bypass header. The
        // body itself must carry the file bytes, the comment, and minorEdit —
        // assert on the multipart payload so a dropped part fails the test.
        Mock::given(method("PUT"))
            .and(path("/wiki/rest/api/content/123/child/attachment"))
            .and(header("X-Atlassian-Token", "nocheck"))
            .and(body_string_contains("name=\"file\""))
            .and(body_string_contains("hello"))
            .and(body_string_contains("name=\"comment\""))
            .and(body_string_contains("v2"))
            .and(body_string_contains("name=\"minorEdit\""))
            .and(body_string_contains("true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "id": "att9" }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"hello").unwrap();

        let client = mock_client(server.uri());
        let result = upload_attachment(
            "123",
            tmp.path().to_str().unwrap(),
            Some("v2"),
            true,
            None,
            &client,
        )
        .await
        .unwrap();
        assert_eq!(result, json!({ "id": "att9" }));
    }

    #[tokio::test]
    async fn integ_upload_attachment_reports_missing_file() {
        let server = MockServer::start().await;
        let client = mock_client(server.uri());
        let err = upload_attachment("123", "/no/such/file.bin", None, false, None, &client)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("Failed to read"), "got: {err}");
    }

    // --- Comment threads --------------------------------------------------

    /// Mount one comment collection. Callers mount an empty collection for
    /// every leaf too — a thread walk still has to ask, and a missing mock
    /// would look like a network failure rather than an unasked question.
    async fn mount_comments(server: &MockServer, at: &str, results: Value) {
        Mock::given(method("GET"))
            .and(path(at.to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": results })))
            .mount(server)
            .await;
    }

    /// Where one comment sits in a listing: its id, depth, the comment it
    /// answers, its family, and the page it belongs to.
    type Placement<'a> = (&'a str, u64, Option<&'a str>, &'a str, Option<&'a str>);

    fn thread_shape(listing: &Value) -> Vec<Placement<'_>> {
        listing["items"]
            .as_array()
            .expect("listing has an items array")
            .iter()
            .map(|comment| {
                (
                    comment["id"].as_str().expect("id"),
                    comment["depth"].as_u64().expect("depth"),
                    comment["parentCommentId"].as_str(),
                    comment["location"].as_str().expect("location"),
                    comment["pageId"].as_str(),
                )
            })
            .collect()
    }

    #[tokio::test]
    async fn integ_get_comments_walks_every_reply_depth_first() {
        let server = MockServer::start().await;
        // r1 ─┬ a1 ─ g1        A page endpoint answers with roots only, so
        //     └ a2             everything below the first row is reachable
        // r2                   solely through the `children` collections.
        mount_comments(
            &server,
            "/wiki/api/v2/pages/5/footer-comments",
            json!([{ "id": "r1", "pageId": "5" }, { "id": "r2", "pageId": "5" }]),
        )
        .await;
        mount_comments(
            &server,
            "/wiki/api/v2/footer-comments/r1/children",
            json!([{ "id": "a1" }, { "id": "a2" }]),
        )
        .await;
        mount_comments(
            &server,
            "/wiki/api/v2/footer-comments/a1/children",
            json!([{ "id": "g1" }]),
        )
        .await;
        for leaf in ["r2", "a2", "g1"] {
            mount_comments(
                &server,
                &format!("/wiki/api/v2/footer-comments/{leaf}/children"),
                json!([]),
            )
            .await;
        }

        let client = mock_client(server.uri());
        let result = get_comments("5", &[CommentFamily::Footer], true, false, &client)
            .await
            .unwrap();

        assert_eq!(
            thread_shape(&result),
            vec![
                ("r1", 0, None, "footer", Some("5")),
                ("a1", 1, Some("r1"), "footer", Some("5")),
                ("g1", 2, Some("a1"), "footer", Some("5")),
                ("a2", 1, Some("r1"), "footer", Some("5")),
                ("r2", 0, None, "footer", Some("5")),
            ]
        );
        // A root answers nothing, and that is stated rather than left out: an
        // absent key and a null one would read the same to a consumer building
        // the tree back up.
        assert!(
            result["items"][0]
                .get("parentCommentId")
                .is_some_and(Value::is_null)
        );
    }

    #[tokio::test]
    async fn integ_get_comments_follows_the_cursor_inside_a_reply_collection() {
        let server = MockServer::start().await;
        // A thread longer than one page is exactly where a single GET would
        // silently drop replies.
        mount_comments(
            &server,
            "/wiki/api/v2/pages/5/footer-comments",
            json!([{ "id": "r1" }]),
        )
        .await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/footer-comments/r1/children"))
            .and(query_param_is_missing("cursor"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "id": "a1" }],
                "_links": { "next": "/wiki/api/v2/footer-comments/r1/children?cursor=P2" }
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/footer-comments/r1/children"))
            .and(query_param("cursor", "P2"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "results": [{ "id": "a2" }] })),
            )
            .expect(1)
            .mount(&server)
            .await;
        for leaf in ["a1", "a2"] {
            mount_comments(
                &server,
                &format!("/wiki/api/v2/footer-comments/{leaf}/children"),
                json!([]),
            )
            .await;
        }

        let client = mock_client(server.uri());
        let result = get_comments("5", &[CommentFamily::Footer], true, false, &client)
            .await
            .unwrap();
        assert_eq!(
            thread_shape(&result),
            vec![
                ("r1", 0, None, "footer", Some("5")),
                ("a1", 1, Some("r1"), "footer", Some("5")),
                ("a2", 1, Some("r1"), "footer", Some("5")),
            ]
        );
    }

    #[tokio::test]
    async fn integ_roots_only_never_asks_for_replies() {
        let server = MockServer::start().await;
        mount_comments(
            &server,
            "/wiki/api/v2/pages/5/footer-comments",
            json!([{ "id": "r1" }]),
        )
        .await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/footer-comments/r1/children"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
            .expect(0)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = get_comments("5", &[CommentFamily::Footer], false, false, &client)
            .await
            .unwrap();
        assert_eq!(
            thread_shape(&result),
            vec![("r1", 0, None, "footer", Some("5"))]
        );
    }

    #[tokio::test]
    async fn integ_a_family_that_was_not_asked_for_is_never_requested() {
        let server = MockServer::start().await;
        mount_comments(
            &server,
            "/wiki/api/v2/pages/5/footer-comments",
            json!([{ "id": "f1" }]),
        )
        .await;
        mount_comments(
            &server,
            "/wiki/api/v2/footer-comments/f1/children",
            json!([]),
        )
        .await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/pages/5/inline-comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
            .expect(0)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        get_comments("5", &[CommentFamily::Footer], true, false, &client)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn integ_both_families_are_listed_and_tagged() {
        let server = MockServer::start().await;
        mount_comments(
            &server,
            "/wiki/api/v2/pages/5/footer-comments",
            json!([{ "id": "f1" }]),
        )
        .await;
        mount_comments(
            &server,
            "/wiki/api/v2/footer-comments/f1/children",
            json!([]),
        )
        .await;
        // The inline family is a whole second set of comments on the same page;
        // listing only the footer one reports a page as quieter than it is.
        mount_comments(
            &server,
            "/wiki/api/v2/pages/5/inline-comments",
            json!([{
                "id": "i1",
                "resolutionStatus": "open",
                "properties": {
                    "inlineMarkerRef": "marker-1",
                    "inlineOriginalSelection": "the highlighted words"
                }
            }]),
        )
        .await;
        mount_comments(
            &server,
            "/wiki/api/v2/inline-comments/i1/children",
            json!([{ "id": "i2" }]),
        )
        .await;
        mount_comments(
            &server,
            "/wiki/api/v2/inline-comments/i2/children",
            json!([]),
        )
        .await;

        let client = mock_client(server.uri());
        let result = get_comments("5", &CommentFamily::ALL, true, false, &client)
            .await
            .unwrap();

        assert_eq!(
            thread_shape(&result),
            vec![
                ("f1", 0, None, "footer", Some("5")),
                ("i1", 0, None, "inline", Some("5")),
                ("i2", 1, Some("i1"), "inline", Some("5")),
            ]
        );
        // What makes an inline comment findable in the page is its anchor, so
        // the response filter must not be treating it as noise.
        assert_eq!(result["items"][1]["resolutionStatus"], json!("open"));
        assert_eq!(
            result["items"][1]["properties"]["inlineOriginalSelection"],
            json!("the highlighted words")
        );
    }

    #[tokio::test]
    async fn integ_a_reply_id_from_the_server_cannot_steer_the_request_path() {
        let server = MockServer::start().await;
        // A page id is user input, but a comment id reaches the path from a
        // RESPONSE — and the walk asks that path for the comment's children.
        // Left raw, the `..` segments below resolve away and the next request
        // lands on `/wiki/pages/999/children`, a path the walk never chose.
        mount_comments(
            &server,
            "/wiki/api/v2/pages/5/footer-comments",
            json!([{ "id": "../../../pages/999" }]),
        )
        .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        get_comments("5", &[CommentFamily::Footer], true, false, &client)
            .await
            .unwrap();

        let requested: Vec<String> = server
            .received_requests()
            .await
            .expect("the mock server recorded its requests")
            .iter()
            .map(|request| request.url.path().to_string())
            .collect();
        assert_eq!(
            requested,
            vec![
                "/wiki/api/v2/pages/5/footer-comments".to_string(),
                "/wiki/api/v2/footer-comments/..%2F..%2F..%2Fpages%2F999/children".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn integ_thread_walk_bails_before_an_endless_supply_of_new_replies() {
        let server = MockServer::start().await;
        // `seen` catches a repeat; nothing catches a server that answers every
        // level with an id it has not used before.
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/pages/5/footer-comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "id": "root" }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(EverFreshReply::default())
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let roots = fetch_all_v2_results(
            &client,
            "get comments",
            &CommentFamily::Footer.page_path("5"),
            &comment_query(),
        )
        .await
        .unwrap();
        let mut walk = ThreadWalk::new(&client, CommentFamily::Footer, Some("5"), true);
        walk.limit = 3;
        let err = walk
            .collect("get comments", roots, None)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("unbounded walk"), "got: {err}");
    }

    #[tokio::test]
    async fn integ_thread_walk_counts_the_replies_it_is_holding_not_only_the_ones_it_took() {
        let server = MockServer::start().await;
        // One level of a wide reply set already outgrows the comments admitted
        // so far, which is why the ceiling counts the queue too.
        mount_comments(
            &server,
            "/wiki/api/v2/pages/5/footer-comments",
            json!([{ "id": "root" }]),
        )
        .await;
        mount_comments(
            &server,
            "/wiki/api/v2/footer-comments/root/children",
            json!(
                (0..10)
                    .map(|n| json!({ "id": format!("r{n}") }))
                    .collect::<Vec<_>>()
            ),
        )
        .await;

        let client = mock_client(server.uri());
        let roots = fetch_all_v2_results(
            &client,
            "get comments",
            &CommentFamily::Footer.page_path("5"),
            &comment_query(),
        )
        .await
        .unwrap();
        let mut walk = ThreadWalk::new(&client, CommentFamily::Footer, Some("5"), true);
        walk.limit = 5;
        let err = walk
            .collect("get comments", roots, None)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("unbounded walk"), "got: {err}");
    }

    /// One reply per request, under an id never used before — the shape `seen`
    /// cannot terminate on.
    #[derive(Default)]
    struct EverFreshReply(std::sync::atomic::AtomicUsize);

    impl wiremock::Respond for EverFreshReply {
        fn respond(&self, _: &wiremock::Request) -> ResponseTemplate {
            let n = self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            ResponseTemplate::new(200)
                .set_body_json(json!({ "results": [{ "id": format!("fresh-{n}") }] }))
        }
    }

    #[tokio::test]
    async fn integ_thread_walk_bails_when_a_comment_answers_itself() {
        let server = MockServer::start().await;
        // A tree cannot reach a comment twice, so this is drift — and a walk
        // that followed it would loop instead of returning a short list.
        mount_comments(
            &server,
            "/wiki/api/v2/pages/5/footer-comments",
            json!([{ "id": "r1" }]),
        )
        .await;
        mount_comments(
            &server,
            "/wiki/api/v2/footer-comments/r1/children",
            json!([{ "id": "r1" }]),
        )
        .await;

        let client = mock_client(server.uri());
        let err = get_comments("5", &[CommentFamily::Footer], true, false, &client)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("reachable from itself"), "unexpected: {err}");
    }

    #[tokio::test]
    async fn integ_get_comments_converts_storage_at_every_depth() {
        let server = MockServer::start().await;
        // `as_markdown=true` converts a comment's storage HTML in place —
        // replies included, or a thread reads half markdown and half HTML.
        mount_comments(
            &server,
            "/wiki/api/v2/pages/5/footer-comments",
            json!([{
                "id": "c1",
                "body": { "storage": { "value": "<p>hello <strong>world</strong></p>" } }
            }]),
        )
        .await;
        mount_comments(
            &server,
            "/wiki/api/v2/footer-comments/c1/children",
            json!([{
                "id": "c2",
                "body": { "storage": { "value": "<p>a <em>reply</em></p>" } }
            }]),
        )
        .await;
        mount_comments(
            &server,
            "/wiki/api/v2/footer-comments/c2/children",
            json!([]),
        )
        .await;

        let client = mock_client(server.uri());
        let result = get_comments("5", &[CommentFamily::Footer], true, true, &client)
            .await
            .unwrap();
        for (index, word) in [(0, "world"), (1, "reply")] {
            let body = result["items"][index]["body"]["storage"]["value"]
                .as_str()
                .unwrap();
            assert!(!body.contains("<p>"), "expected HTML stripped, got: {body}");
            assert!(body.contains(word), "expected text preserved, got: {body}");
        }
    }

    #[tokio::test]
    async fn integ_get_comment_reads_only_the_named_family() {
        let server = MockServer::start().await;
        // The family is part of the address. Falling back to the other one on a
        // 404 would make "wrong family", "deleted" and "not visible to you" the
        // same answer.
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/inline-comments/77"))
            .and(query_param("body-format", "storage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "77",
                "body": { "storage": { "value": "<p>anchored</p>" } }
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/footer-comments/77"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "77" })))
            .expect(0)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = get_comment("77", CommentFamily::Inline, false, &client)
            .await
            .unwrap();
        assert_eq!(result["id"], json!("77"));
        assert_eq!(result["location"], json!("inline"));
    }

    #[tokio::test]
    async fn integ_get_comment_encodes_the_id() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/footer-comments/7%2F7"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "7/7" })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = get_comment("7/7", CommentFamily::Footer, false, &client)
            .await
            .unwrap();
        assert_eq!(result["id"], json!("7/7"));
    }

    #[tokio::test]
    async fn integ_replies_are_rooted_under_the_comment_that_was_asked_for() {
        let server = MockServer::start().await;
        mount_comments(
            &server,
            "/wiki/api/v2/footer-comments/r1/children",
            json!([{ "id": "a1" }]),
        )
        .await;
        mount_comments(
            &server,
            "/wiki/api/v2/footer-comments/a1/children",
            json!([{ "id": "g1" }]),
        )
        .await;
        mount_comments(
            &server,
            "/wiki/api/v2/footer-comments/g1/children",
            json!([]),
        )
        .await;

        let client = mock_client(server.uri());
        let result = get_comment_replies("r1", CommentFamily::Footer, false, &client)
            .await
            .unwrap();
        // The origin is not a member of its own reply listing, and no page id
        // is claimed — a walk that began at a comment was never told one.
        assert_eq!(
            thread_shape(&result),
            vec![
                ("a1", 0, Some("r1"), "footer", None),
                ("g1", 1, Some("a1"), "footer", None),
            ]
        );
    }

    // --- v2 cursor pagination --------------------------------------------

    #[tokio::test]
    async fn integ_list_follows_relative_next_cursor() {
        let server = MockServer::start().await;
        // Page 1 (no cursor) hands back a relative `_links.next`; the helper must
        // follow it and accumulate, not stop at the first page.
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/spaces"))
            .and(query_param_is_missing("cursor"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "id": "1", "key": "A" }],
                "_links": { "next": "/wiki/api/v2/spaces?cursor=P2" }
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/spaces"))
            .and(query_param("cursor", "P2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "id": "2", "key": "B" }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = get_spaces(&client).await.unwrap();
        let keys: Vec<&str> = result["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["key"].as_str().unwrap())
            .collect();
        assert_eq!(keys, vec!["A", "B"]);
    }

    #[tokio::test]
    async fn integ_list_follows_absolute_next_cursor_on_the_configured_host() {
        let server = MockServer::start().await;
        // An absolute `_links.next` contributes its path only. Here it names a
        // host the client was never configured with: pagination must continue
        // against the mock server, so the second page is served and the
        // credential is never offered to the host the response named.
        let next = "https://elsewhere.invalid/wiki/api/v2/pages/9/labels?cursor=P2".to_string();
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/pages/9/labels"))
            .and(query_param_is_missing("cursor"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "name": "a" }],
                "_links": { "next": next }
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/pages/9/labels"))
            .and(query_param("cursor", "P2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "name": "b" }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = get_labels("9", &client).await.unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 2);
        assert_eq!(result["items"][1]["name"], "b");
    }

    #[tokio::test]
    async fn integ_v1_search_all_bails_when_the_cursor_stops_advancing() {
        let server = MockServer::start().await;
        // v1 walks the same cursor forever unless the trail stops it.
        Mock::given(method("GET"))
            .and(path("/wiki/rest/api/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "content": { "id": "1", "title": "a" } }],
                "totalSize": 99,
                "_links": {
                    "base": format!("{}/wiki", server.uri()),
                    "next": "/rest/api/search?cursor=stuck"
                }
            })))
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let err = search_all("type = page", 50, None, None, false, false, &client)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("did not advance"), "unexpected error: {err}");
    }

    #[tokio::test]
    async fn integ_list_bails_when_only_the_discarded_prefix_advances() {
        let server = MockServer::start().await;
        // Same resolved path every time, dressed in a different host. The
        // request never advances, so the guard must catch it — which it only
        // can because it keys on the path rather than on the raw link.
        let mut hosts = ["https://a.invalid", "https://b.invalid"].iter().cycle();
        for _ in 0..2 {
            let next = format!(
                "{}/wiki/api/v2/pages/9/labels?cursor=P2",
                hosts.next().unwrap()
            );
            Mock::given(method("GET"))
                .and(path("/wiki/api/v2/pages/9/labels"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "results": [{ "name": "a" }],
                    "_links": { "next": next }
                })))
                .up_to_n_times(1)
                .mount(&server)
                .await;
        }

        let client = mock_client(server.uri());
        let err = get_labels("9", &client).await.unwrap_err().to_string();
        assert!(err.contains("did not advance"), "unexpected error: {err}");
    }

    #[tokio::test]
    async fn integ_list_bails_on_a_pathless_next_instead_of_truncating() {
        let server = MockServer::start().await;
        // A `next` that carries no path is schema drift. Returning the first
        // page as if it were the whole collection is the failure this helper
        // exists to prevent, so it must surface as an error.
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/pages/9/labels"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "name": "a" }],
                "_links": { "next": "https://elsewhere.invalid" }
            })))
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let err = get_labels("9", &client).await.unwrap_err().to_string();
        assert!(err.contains("no path"), "unexpected error: {err}");
    }

    #[tokio::test]
    async fn integ_get_comments_sends_body_format_then_accumulates() {
        let server = MockServer::start().await;
        // The `body-format` and `limit` queries ride only on the first request;
        // the cursor link carries its own params on the follow-up.
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/pages/5/footer-comments"))
            .and(query_param("body-format", "storage"))
            .and(query_param("limit", COMMENT_PAGE_SIZE))
            .and(query_param_is_missing("cursor"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "id": "c1" }],
                "_links": { "next": "/wiki/api/v2/pages/5/footer-comments?cursor=P2" }
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/pages/5/footer-comments"))
            .and(query_param("cursor", "P2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "id": "c2" }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = get_comments("5", &[CommentFamily::Footer], false, false, &client)
            .await
            .unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn integ_pagination_bails_on_non_advancing_cursor() {
        let server = MockServer::start().await;
        // A cursor that points back at itself must terminate with an error
        // rather than looping forever.
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/spaces"))
            .and(query_param_is_missing("cursor"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "id": "1" }],
                "_links": { "next": "/wiki/api/v2/spaces?cursor=LOOP" }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/spaces"))
            .and(query_param("cursor", "LOOP"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "id": "2" }],
                "_links": { "next": "/wiki/api/v2/spaces?cursor=LOOP" }
            })))
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let err = get_spaces(&client).await.unwrap_err().to_string();
        assert!(err.contains("did not advance"), "got: {err}");
    }

    #[tokio::test]
    async fn integ_pagination_bails_on_missing_results() {
        let server = MockServer::start().await;
        // A 2xx page without a `results` array is anomalous — surface it loudly
        // instead of silently returning an empty list.
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/pages/7/labels"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let err = get_labels("7", &client).await.unwrap_err().to_string();
        assert!(err.contains("no 'results' array"), "got: {err}");
    }
}
