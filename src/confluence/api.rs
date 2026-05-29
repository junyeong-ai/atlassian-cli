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
    let url = format!(
        "/wiki/api/v2/pages/{}/children",
        encode_path_segment(page_id)
    );

    let response = client
        .get(Service::Confluence, &url)
        .await?
        .header("Accept", "application/json")
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to get child pages ({}): {}", status, body);
    }

    let mut data: Value = response.json().await?;
    filter::apply(&mut data, client.config());

    Ok(json!({"items": data["results"]}))
}

pub async fn get_comments(page_id: &str, as_markdown: bool, client: &ApiClient) -> Result<Value> {
    let url = format!(
        "/wiki/api/v2/pages/{}/footer-comments",
        encode_path_segment(page_id)
    );

    let response = client
        .get(Service::Confluence, &url)
        .await?
        .header("Accept", "application/json")
        .query(&[("body-format", "storage")])
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to get comments ({}): {}", status, body);
    }

    let mut data: Value = response.json().await?;
    filter::apply(&mut data, client.config());

    if as_markdown {
        convert_comments_to_markdown(&mut data);
    }

    Ok(json!({"items": data["results"]}))
}

pub async fn create_page(
    space_key: &str,
    title: &str,
    content: &str,
    include_all_fields: Option<bool>,
    additional_includes: Option<Vec<String>>,
    client: &ApiClient,
) -> Result<Value> {
    // First, convert space_key to space_id using v2 API
    let space_url = "/wiki/api/v2/spaces";

    let space_response = client
        .get(Service::Confluence, space_url)
        .await?
        .query(&[("keys", space_key)])
        .header("Accept", "application/json")
        .send()
        .await?;

    if !space_response.status().is_success() {
        let status = space_response.status();
        let body = space_response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to get space '{}' ({}): {}", space_key, status, body);
    }

    let space_data: Value = space_response.json().await?;
    let space_id = space_data["results"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|space| space["id"].as_str())
        .ok_or_else(|| anyhow::anyhow!("Space '{}' not found", space_key))?;

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
    // First, get the current page to get the version number using v2 API
    let encoded_page_id = encode_path_segment(page_id);
    let get_url = format!("/wiki/api/v2/pages/{}", encoded_page_id);

    let get_response = client
        .get(Service::Confluence, &get_url)
        .await?
        .header("Accept", "application/json")
        .query(&[("include-version", "true")])
        .send()
        .await?;

    if !get_response.status().is_success() {
        let status = get_response.status();
        let body = get_response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to get page for update ({}): {}", status, body);
    }

    let current_page: Value = get_response.json().await?;
    let current_version = current_page["version"]["number"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("Failed to get current version"))?;

    // Now update the page with v2 API
    let update_url = format!("/wiki/api/v2/pages/{}", encoded_page_id);

    let query_params = apply_v2_filtering(include_all_fields, additional_includes);

    let body = json!({
        "id": page_id,
        "title": title,
        "body": {
            "representation": "storage",
            "value": content
        },
        "version": {
            "number": current_version + 1
        }
    });

    let response = client
        .put(Service::Confluence, &update_url)
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

fn convert_comments_to_markdown(data: &mut Value) {
    let Some(results) = data.get_mut("results").and_then(|r| r.as_array_mut()) else {
        return;
    };
    for item in results {
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
    use wiremock::matchers::{method, path, query_param};
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

    #[test]
    fn test_create_page_body_format() {
        let title = "Test Page";
        let content = "<p>Test content</p>";
        let space_id = "space123";

        let body = json!({
            "spaceId": space_id,
            "title": title,
            "body": {
                "representation": "storage",
                "value": content
            }
        });

        assert_eq!(body["spaceId"], "space123");
        assert_eq!(body["title"], "Test Page");
        assert_eq!(body["body"]["representation"], "storage");
        assert_eq!(body["body"]["value"], "<p>Test content</p>");
    }

    #[test]
    fn test_update_page_body_format() {
        let page_id = "12345";
        let title = "Updated Title";
        let content = "<p>Updated content</p>";
        let current_version = 5u64;

        let body = json!({
            "id": page_id,
            "title": title,
            "body": {
                "representation": "storage",
                "value": content
            },
            "version": {
                "number": current_version + 1
            }
        });

        assert_eq!(body["id"], "12345");
        assert_eq!(body["title"], "Updated Title");
        assert_eq!(body["body"]["representation"], "storage");
        assert_eq!(body["body"]["value"], "<p>Updated content</p>");
        assert_eq!(body["version"]["number"], 6);
    }
}
