use crate::client::{ApiClient, Service};
use crate::config::Config;
use crate::confluence::fields::{apply_v2_filtering, build_search_expand};
use crate::filter;
use crate::http_utils::encode_path_segment;
use crate::markdown::confluence_to_markdown;
use crate::query_utils::inject_filter;
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

/// Matches `space` as a whole word followed by a CQL comparison operator
/// (`=`, `!=`, `in (...)`, `not in (...)`). The word boundary prevents
/// false positives on identifiers ending in "space" (e.g. `mySpace = X`),
/// matching the same defensive posture used for `PROJECT_CLAUSE_RE` in
/// the Jira layer.
static SPACE_CLAUSE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bspace\s*(?:=|!=|not\s+in\s*\(|in\s*\()").unwrap());

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

/// Combine a Confluence pagination envelope's `_links.base` and `_links.next`
/// into the URL of the next page. Both inputs are server-supplied and never
/// contain user-controlled segments — they bypass `encode_path_segment` for
/// that reason. The result is later normalized via `client.rewrite_url` so
/// service-account auth still routes through the proxy host.
fn build_next_url(links_base: &str, next_path: &str) -> String {
    if next_path.starts_with("http") {
        next_path.to_string()
    } else {
        // `links_base` from the API response already includes `/wiki`.
        format!("{}{}", links_base, next_path)
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

    let response = client
        .get(Service::Confluence, url)
        .await?
        .header("Accept", "application/json")
        .query(&[
            ("cql", final_cql.as_str()),
            ("limit", &effective_limit.to_string()),
            ("expand", &expand),
        ])
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Search failed ({}): {}", status, body);
    }

    let mut data: Value = response.json().await?;

    let items = extract_content_from_results(&mut data, as_markdown);
    let total = data["totalSize"].as_u64().unwrap_or(items.len() as u64);

    let mut output = json!({
        "items": items,
        "count": items.len(),
        "total": total
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

    let mut all_items: Vec<Value> = Vec::new();
    let mut page_num = 1;
    let mut next_url: Option<String> = None;
    let mut total_size: u64 = 0;

    loop {
        let mut data = if let Some(ref url) = next_url {
            fetch_page(client, url).await?
        } else {
            fetch_initial_page(client, &final_cql, &expand, limit).await?
        };

        if page_num == 1 {
            total_size = data["totalSize"].as_u64().unwrap_or(0);
        }

        let mut items = extract_content_from_results(&mut data, as_markdown);
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

        // _links.next is our signal to continue paginating; absence means we're done.
        // `let-else` keeps the happy path flat and removes the unwraps below.
        let Some(next_path) = data["_links"]["next"].as_str() else {
            break;
        };
        if count == 0 {
            break;
        }

        // _links.base is the site URL (e.g. "https://domain.atlassian.net/wiki").
        // If missing, next_path must already be absolute.
        let raw_url = match data["_links"]["base"].as_str() {
            Some(base) => build_next_url(base, next_path),
            None => next_path.to_string(),
        };
        next_url = Some(client.rewrite_url(Service::Confluence, &raw_url));

        page_num += 1;
        sleep(Duration::from_millis(
            client.config().performance.rate_limit_delay_ms,
        ))
        .await;
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

    let response = client
        .get(Service::Confluence, url)
        .await?
        .header("Accept", "application/json")
        .query(&[
            ("cql", cql),
            ("limit", &effective_limit),
            ("expand", expand),
        ])
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Search failed ({}): {}", status, body);
    }

    response.json().await.map_err(Into::into)
}

async fn fetch_page(client: &ApiClient, url: &str) -> Result<Value> {
    let response = client
        .get_absolute(url)
        .await?
        .header("Accept", "application/json")
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Search failed ({}): {}", status, body);
    }

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

    let response = client
        .get(Service::Confluence, &url)
        .await?
        .header("Accept", "application/json")
        .query(&query_params)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to get page ({}): {}", status, body);
    }

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

pub async fn get_comments(page_id: &str, as_markdown: bool, client: &ApiClient) -> Result<Value> {
    let path = format!(
        "/wiki/api/v2/pages/{}/footer-comments",
        encode_path_segment(page_id)
    );
    let items =
        fetch_all_v2_results(client, "get comments", &path, &[("body-format", "storage")]).await?;

    let mut envelope = v2_list_envelope(items, client);
    if as_markdown && let Some(comments) = envelope["items"].as_array_mut() {
        convert_comments_to_markdown(comments);
    }
    Ok(envelope)
}

pub async fn create_page(
    space_key: &str,
    title: &str,
    content: &str,
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

    let body = json!({
        "spaceId": space_id,
        "title": title,
        "body": {
            "representation": "storage",
            "value": content
        }
    });

    let response = client
        .post(Service::Confluence, url)
        .await?
        .header("Content-Type", "application/json")
        .query(&query_params)
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to create page ({}): {}", status, body);
    }

    let data: Value = response.json().await?;
    Ok(json!({
        "id": data["id"],
        "title": data["title"]
    }))
}

pub async fn update_page(
    page_id: &str,
    title: &str,
    content: &str,
    include_all_fields: Option<bool>,
    additional_includes: Option<Vec<String>>,
    client: &ApiClient,
) -> Result<Value> {
    let url = format!("/wiki/api/v2/pages/{}", encode_path_segment(page_id));
    let next_version = fetch_version_number(client, &url).await? + 1;

    let query_params = apply_v2_filtering(include_all_fields, additional_includes);

    // `status: "current"` is part of the v2 update contract and keeps the page
    // published; this CLI only edits live pages, so it is always "current".
    let body = json!({
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

    let response = client
        .put(Service::Confluence, &url)
        .await?
        .header("Content-Type", "application/json")
        .query(&query_params)
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to update page ({}): {}", status, body);
    }

    let data: Value = response.json().await?;
    Ok(json!({
        "id": data["id"],
        "version": data["version"]["number"]
    }))
}

/// Move a page to the Confluence trash (v2 `DELETE` is recoverable, unlike
/// Jira issue deletion). Still a whole-resource destruction, so the CLI layer
/// requires an explicit `--yes`.
pub async fn delete_page(page_id: &str, client: &ApiClient) -> Result<Value> {
    let url = format!("/wiki/api/v2/pages/{}", encode_path_segment(page_id));

    let response = client
        .delete(Service::Confluence, &url)
        .await?
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to delete page ({}): {}", status, body);
    }

    Ok(json!({}))
}

/// Fetch a single space object by key via the v2 spaces endpoint (`?keys=`),
/// or `None` when no space matches. Returns the raw API object **without**
/// `filter::apply` so each caller decides: `resolve_space_id` reads the
/// unfiltered `id`; `get_space` applies the field filter before handing the
/// object to the user.
async fn fetch_space_by_key(space_key: &str, client: &ApiClient) -> Result<Option<Value>> {
    let response = client
        .get(Service::Confluence, "/wiki/api/v2/spaces")
        .await?
        .header("Accept", "application/json")
        .query(&[("keys", space_key)])
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to get space '{}' ({}): {}", space_key, status, body);
    }

    let data: Value = response.json().await?;
    Ok(data["results"]
        .as_array()
        .and_then(|arr| arr.first())
        .cloned())
}

/// Resolve a Confluence space key to its numeric space id. The single
/// space-key→id lookup, shared by `create_page` and the `space` commands.
pub async fn resolve_space_id(space_key: &str, client: &ApiClient) -> Result<String> {
    fetch_space_by_key(space_key, client)
        .await?
        .and_then(|space| space["id"].as_str().map(|s| s.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Space '{}' not found", space_key))
}

/// Read the current `version.number` of a versioned v2 resource (page or
/// comment). v2 writes that bump a resource require the next version number, so
/// every updater reads the current one through this single helper instead of
/// re-implementing the GET. `include-version=true` is requested explicitly
/// because the page endpoint omits the version object otherwise; endpoints that
/// always include it ignore the redundant query param.
async fn fetch_version_number(client: &ApiClient, url: &str) -> Result<u64> {
    let response = client
        .get(Service::Confluence, url)
        .await?
        .header("Accept", "application/json")
        .query(&[("include-version", "true")])
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to fetch current version ({}): {}", status, body);
    }

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
/// carries the original query (cursor, limit, `body-format`, …). A relative
/// `next` (`/wiki/…`) is re-issued against the service base; an absolute one is
/// routed through `rewrite_url` so proxy/service-account auth follows it to the
/// correct host.
async fn fetch_all_v2_results(
    client: &ApiClient,
    what: &str,
    path: &str,
    query: &[(&str, &str)],
) -> Result<Vec<Value>> {
    let mut results: Vec<Value> = Vec::new();
    let mut next: Option<String> = None;
    let mut seen: HashSet<String> = HashSet::new();

    loop {
        let request = match next.as_deref() {
            None => client.get(Service::Confluence, path).await?.query(query),
            Some(n) if n.starts_with("http") => {
                client
                    .get_absolute(&client.rewrite_url(Service::Confluence, n))
                    .await?
            }
            Some(n) => client.get(Service::Confluence, n).await?,
        };

        let response = request.header("Accept", "application/json").send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to {} ({}): {}", what, status, body);
        }

        // A v2 list page always carries a `results` array; its absence on a 2xx
        // means schema drift or a wrong-shaped response. Bail rather than
        // silently returning a short list — the same posture as the Jira-side
        // `paginate_values` helper.
        let data: Value = response.json().await?;
        let page = data["results"].as_array().ok_or_else(|| {
            anyhow::anyhow!("Failed to {}: response had no 'results' array", what)
        })?;
        results.extend(page.iter().cloned());

        match data["_links"]["next"].as_str() {
            Some(n) if !n.is_empty() => {
                // A cursor that repeats a URL we've already fetched is not
                // advancing — bail instead of looping forever.
                if !seen.insert(n.to_string()) {
                    anyhow::bail!("Failed to {}: pagination cursor did not advance", what);
                }
                next = Some(n.to_string());
            }
            _ => break,
        }
    }

    Ok(results)
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
    // interpolated into a path) so they need no path encoding.
    let mut request_body = json!({
        "pageId": page_id,
        "body": {
            "representation": "storage",
            "value": body,
        },
    });
    if let Some(parent) = parent_id {
        request_body["parentCommentId"] = json!(parent);
    }

    let response = client
        .post(Service::Confluence, "/wiki/api/v2/footer-comments")
        .await?
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to add comment ({}): {}", status, body);
    }

    let data: Value = response.json().await?;
    Ok(json!({"id": data["id"]}))
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

    let response = client
        .put(Service::Confluence, &url)
        .await?
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to update comment ({}): {}", status, body);
    }

    let data: Value = response.json().await?;
    Ok(json!({"id": data["id"]}))
}

/// Delete a footer comment by id. The id is the specificity guard, so — like
/// the Jira `delete_comment`/`remove_link` family — no `--yes` is required.
pub async fn delete_comment(comment_id: &str, client: &ApiClient) -> Result<Value> {
    let url = format!(
        "/wiki/api/v2/footer-comments/{}",
        encode_path_segment(comment_id)
    );

    let response = client
        .delete(Service::Confluence, &url)
        .await?
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to delete comment ({}): {}", status, body);
    }

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

    let response = client
        .post(Service::Confluence, &url)
        .await?
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to add label ({}): {}", status, body);
    }

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

    let response = client
        .delete(Service::Confluence, &url)
        .await?
        .query(&[("name", label)])
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to remove label ({}): {}", status, body);
    }

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

    let response = if let Some(prop) = existing {
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
            .send()
            .await?
    } else {
        let request_body = json!({ "key": key, "value": value });
        client
            .post(Service::Confluence, &collection_url)
            .await?
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?
    };

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to set property ({}): {}", status, body);
    }

    let data: Value = response.json().await?;
    Ok(json!({"id": data["id"]}))
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

    let response = client
        .delete(Service::Confluence, &url)
        .await?
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to delete property ({}): {}", status, body);
    }

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
    let response = client
        .get(Service::Confluence, collection_url)
        .await?
        .header("Accept", "application/json")
        .query(&[("key", key)])
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to look up property ({}): {}", status, body);
    }

    let data: Value = response.json().await?;
    Ok(data["results"]
        .as_array()
        .and_then(|arr| arr.first())
        .cloned())
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

    // `minorEdit` is always sent (the v1 endpoint expects it); `true` suppresses
    // the watcher notification that a re-upload would otherwise fire.
    let part = reqwest::multipart::Part::bytes(bytes).file_name(file_name);
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

    let response = client
        .put(Service::Confluence, &url)
        .await?
        .header("X-Atlassian-Token", "nocheck")
        .multipart(form)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to upload attachment ({}): {}", status, body);
    }

    // v1 wraps the created/updated attachment in a `results` array.
    let data: Value = response.json().await?;
    Ok(json!({"id": data["results"][0]["id"]}))
}

fn extract_content_from_results(data: &mut Value, as_markdown: bool) -> Vec<Value> {
    let Some(results) = data.get_mut("results").and_then(|r| r.as_array_mut()) else {
        return vec![];
    };

    results
        .iter_mut()
        .filter_map(|item| {
            let mut content = item.get_mut("content")?.take();

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

            Some(content)
        })
        .collect()
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

    #[test]
    fn test_build_next_url_relative_path() {
        let links_base = "https://test.atlassian.net/wiki";
        let next_path = "/rest/api/search?cql=type%3Dpage&cursor=abc123";
        let result = build_next_url(links_base, next_path);
        assert_eq!(
            result,
            "https://test.atlassian.net/wiki/rest/api/search?cql=type%3Dpage&cursor=abc123"
        );
    }

    #[test]
    fn test_build_next_url_absolute() {
        let base_url = "https://test.atlassian.net/wiki";
        let next_path = "https://other.atlassian.net/wiki/rest/api/search?cursor=xyz";
        let result = build_next_url(base_url, next_path);
        assert_eq!(
            result,
            "https://other.atlassian.net/wiki/rest/api/search?cursor=xyz"
        );
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
        let result = create_page("ENG", "Spec", "<p>x</p>", None, None, &client)
            .await
            .unwrap();
        assert_eq!(result, json!({ "id": "pid", "title": "Spec" }));
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
        let result = update_page("12345", "Updated", "<p>y</p>", None, None, &client)
            .await
            .unwrap();
        assert_eq!(result, json!({ "id": "12345", "version": 6 }));
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
            .and(body_json(json!({
                "pageId": "123",
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
        let err = upload_attachment("123", "/no/such/file.bin", None, false, &client)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("Failed to read"), "got: {err}");
    }

    #[tokio::test]
    async fn integ_get_comments_converts_storage_to_markdown() {
        let server = MockServer::start().await;
        // `as_markdown=true` must convert each comment's storage HTML in place.
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/pages/5/footer-comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{
                    "id": "c1",
                    "body": { "storage": { "value": "<p>hello <strong>world</strong></p>" } }
                }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = get_comments("5", true, &client).await.unwrap();
        let body = result["items"][0]["body"]["storage"]["value"]
            .as_str()
            .unwrap();
        assert!(!body.contains("<p>"), "expected HTML stripped, got: {body}");
        assert!(
            body.contains("world"),
            "expected text preserved, got: {body}"
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
    async fn integ_list_follows_absolute_next_cursor() {
        let server = MockServer::start().await;
        // When the API returns an absolute `_links.next`, the helper routes it
        // through `rewrite_url` + `get_absolute` (the `starts_with("http")` arm).
        let next = format!("{}/wiki/api/v2/pages/9/labels?cursor=P2", server.uri());
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
    async fn integ_get_comments_sends_body_format_then_accumulates() {
        let server = MockServer::start().await;
        // The `body-format` query rides only on the first request; the cursor
        // link carries its own params on the follow-up.
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/pages/5/footer-comments"))
            .and(query_param("body-format", "storage"))
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
        let result = get_comments("5", false, &client).await.unwrap();
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
