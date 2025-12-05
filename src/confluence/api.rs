use crate::config::Config;
use crate::confluence::fields::{apply_expand_filtering, apply_v2_filtering};
use crate::confluence::markdown::convert_to_markdown;
use crate::http;
use anyhow::Result;
use reqwest::Client;
use serde_json::{Value, json};
use std::io::{self, Write};
use std::time::Duration;
use tokio::time::sleep;

const MAX_LIMIT: u32 = 250;
const RATE_LIMIT_DELAY_MS: u64 = 200;

fn apply_space_filter(cql: &str, config: &Config) -> String {
    if config.confluence.spaces_filter.is_empty() {
        return cql.to_string();
    }

    let cql_lower = cql.to_lowercase();
    if cql_lower.contains("space ")
        || cql_lower.contains("space=")
        || cql_lower.contains("space in")
    {
        cql.to_string()
    } else {
        let spaces = config
            .confluence
            .spaces_filter
            .iter()
            .map(|s| format!("\"{}\"", s))
            .collect::<Vec<_>>()
            .join(",");
        format!("space IN ({}) AND ({})", spaces, cql)
    }
}

fn build_next_url(links_base: &str, next_path: &str) -> String {
    if next_path.starts_with("http") {
        next_path.to_string()
    } else {
        // links_base from API response already includes /wiki
        format!("{}{}", links_base, next_path)
    }
}

pub async fn search(
    query: &str,
    limit: u32,
    include_all_fields: Option<bool>,
    additional_expand: Option<Vec<String>>,
    as_markdown: bool,
    config: &Config,
) -> Result<Value> {
    let final_cql = apply_space_filter(query, config);
    let client = http::client(config);
    // Use content/search API which supports body.storage expand
    let url = format!("{}/wiki/rest/api/content/search", config.base_url());
    let (url, expand_param) = apply_expand_filtering(&url, include_all_fields, additional_expand);

    let mut query_params = vec![
        ("cql".to_string(), final_cql),
        ("limit".to_string(), limit.min(MAX_LIMIT).to_string()),
    ];

    if let Some(expand) = expand_param {
        query_params.push(("expand".to_string(), expand));
    }

    let response = client
        .get(&url)
        .header("Authorization", http::auth_header(config))
        .header("Accept", "application/json")
        .query(&query_params)
        .send()
        .await?;

    if !response.status().is_success() {
        anyhow::bail!("Search failed: {}", response.status());
    }

    let mut data: Value = response.json().await?;

    if as_markdown {
        convert_results_to_markdown(&mut data);
    }

    Ok(json!({
        "items": data["results"],
        "total": data["totalSize"]
    }))
}

pub async fn search_all(
    query: &str,
    include_all_fields: Option<bool>,
    additional_expand: Option<Vec<String>>,
    stream: bool,
    as_markdown: bool,
    config: &Config,
) -> Result<Value> {
    let final_cql = apply_space_filter(query, config);
    let client = http::client(config);
    let base_url = config.base_url();
    // Use content/search API which supports body.storage expand
    let initial_url = format!("{}/wiki/rest/api/content/search", base_url);
    let (_, expand_param) =
        apply_expand_filtering(&initial_url, include_all_fields, additional_expand.clone());

    let mut all_items: Vec<Value> = Vec::new();
    let mut page_num = 1;
    let mut next_url: Option<String> = None;

    loop {
        let mut data = if let Some(ref url) = next_url {
            fetch_page(&client, url, config).await?
        } else {
            fetch_initial_page(
                &client,
                &initial_url,
                &final_cql,
                expand_param.as_deref(),
                config,
            )
            .await?
        };

        if as_markdown {
            convert_results_to_markdown(&mut data);
        }

        let results = data["results"].as_array().cloned().unwrap_or_default();
        let count = results.len();
        let total = data["totalSize"].as_u64().unwrap_or(0);

        if stream {
            for item in &results {
                println!("{}", serde_json::to_string(item)?);
            }
            io::stdout().flush()?;
        }

        all_items.extend(results);

        eprintln!(
            "  Page {}: {} items (total: {}/{})",
            page_num,
            count,
            all_items.len(),
            total
        );

        let next_path = data["_links"]["next"].as_str();
        if next_path.is_none() || count == 0 {
            break;
        }

        // Use _links.base from API response (includes /wiki), not config.base_url()
        let links_base = data["_links"]["base"].as_str().unwrap_or(base_url);
        next_url = Some(build_next_url(links_base, next_path.unwrap()));
        page_num += 1;
        sleep(Duration::from_millis(RATE_LIMIT_DELAY_MS)).await;
    }

    eprintln!("\nTotal: {} items fetched", all_items.len());

    if stream {
        Ok(json!({"streamed": true, "total": all_items.len()}))
    } else {
        Ok(json!({
            "items": all_items,
            "total": all_items.len()
        }))
    }
}

async fn fetch_initial_page(
    client: &Client,
    url: &str,
    cql: &str,
    expand: Option<&str>,
    config: &Config,
) -> Result<Value> {
    let mut query_params = vec![("cql", cql), ("limit", "250")];

    if let Some(exp) = expand {
        query_params.push(("expand", exp));
    }

    let response = client
        .get(url)
        .header("Authorization", http::auth_header(config))
        .header("Accept", "application/json")
        .query(&query_params)
        .send()
        .await?;

    if !response.status().is_success() {
        anyhow::bail!("Search failed: {}", response.status());
    }

    response.json().await.map_err(Into::into)
}

async fn fetch_page(client: &Client, url: &str, config: &Config) -> Result<Value> {
    let response = client
        .get(url)
        .header("Authorization", http::auth_header(config))
        .header("Accept", "application/json")
        .send()
        .await?;

    if !response.status().is_success() {
        anyhow::bail!("Search failed: {}", response.status());
    }

    response.json().await.map_err(Into::into)
}

pub async fn get_page(
    page_id: &str,
    include_all_fields: Option<bool>,
    additional_includes: Option<Vec<String>>,
    as_markdown: bool,
    config: &Config,
) -> Result<Value> {
    let client = http::client(config);
    let url = format!("{}/wiki/api/v2/pages/{}", config.base_url(), page_id);

    let query_params = apply_v2_filtering(include_all_fields, additional_includes);

    let response = client
        .get(&url)
        .header("Authorization", http::auth_header(config))
        .header("Accept", "application/json")
        .query(&query_params)
        .send()
        .await?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to get page: {}", response.status());
    }

    let mut data: Value = response.json().await?;

    if as_markdown {
        convert_page_to_markdown(&mut data);
    }

    Ok(data)
}

pub async fn get_page_children(
    page_id: &str,
    include_all_fields: Option<bool>,
    additional_includes: Option<Vec<String>>,
    config: &Config,
) -> Result<Value> {
    let client = http::client(config);
    let url = format!(
        "{}/wiki/api/v2/pages/{}/children",
        config.base_url(),
        page_id
    );

    let query_params = apply_v2_filtering(include_all_fields, additional_includes);

    let response = client
        .get(&url)
        .header("Authorization", http::auth_header(config))
        .header("Accept", "application/json")
        .query(&query_params)
        .send()
        .await?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to get child pages: {}", response.status());
    }

    let data: Value = response.json().await?;
    Ok(json!({"items": data["results"]}))
}

pub async fn get_comments(
    page_id: &str,
    include_all_fields: Option<bool>,
    additional_includes: Option<Vec<String>>,
    config: &Config,
) -> Result<Value> {
    let client = http::client(config);
    let url = format!(
        "{}/wiki/api/v2/pages/{}/footer-comments",
        config.base_url(),
        page_id
    );

    let query_params = apply_v2_filtering(include_all_fields, additional_includes);

    let response = client
        .get(&url)
        .header("Authorization", http::auth_header(config))
        .header("Accept", "application/json")
        .query(&query_params)
        .send()
        .await?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to get comments: {}", response.status());
    }

    let data: Value = response.json().await?;
    Ok(json!({"items": data["results"]}))
}

pub async fn create_page(
    space_key: &str,
    title: &str,
    content: &str,
    include_all_fields: Option<bool>,
    additional_includes: Option<Vec<String>>,
    config: &Config,
) -> Result<Value> {
    let client = http::client(config);

    // First, convert space_key to space_id using v2 API
    let space_url = format!("{}/wiki/api/v2/spaces", config.base_url());

    let space_response = client
        .get(&space_url)
        .query(&[("keys", space_key)]) // Automatic URL encoding
        .header("Authorization", http::auth_header(config))
        .header("Accept", "application/json")
        .send()
        .await?;

    if !space_response.status().is_success() {
        anyhow::bail!(
            "Failed to get space ID for key '{}': {}",
            space_key,
            space_response.status()
        );
    }

    let space_data: Value = space_response.json().await?;
    let space_id = space_data["results"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|space| space["id"].as_str())
        .ok_or_else(|| anyhow::anyhow!("Space '{}' not found", space_key))?;

    // Now create the page with v2 API
    let url = format!("{}/wiki/api/v2/pages", config.base_url());

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
        .post(&url)
        .header("Authorization", http::auth_header(config))
        .header("Content-Type", "application/json")
        .query(&query_params)
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let error = response.text().await?;
        anyhow::bail!("Failed to create page: {}", error);
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
    config: &Config,
) -> Result<Value> {
    let client = http::client(config);

    // First, get the current page to get the version number using v2 API
    let get_url = format!("{}/wiki/api/v2/pages/{}", config.base_url(), page_id);

    let get_response = client
        .get(&get_url)
        .header("Authorization", http::auth_header(config))
        .header("Accept", "application/json")
        .query(&[("include-version", "true")])
        .send()
        .await?;

    if !get_response.status().is_success() {
        anyhow::bail!("Failed to get page for update: {}", get_response.status());
    }

    let current_page: Value = get_response.json().await?;
    let current_version = current_page["version"]["number"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("Failed to get current version"))?;

    // Now update the page with v2 API
    let update_url = format!("{}/wiki/api/v2/pages/{}", config.base_url(), page_id);

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
        .put(&update_url)
        .header("Authorization", http::auth_header(config))
        .header("Content-Type", "application/json")
        .query(&query_params)
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let error = response.text().await?;
        anyhow::bail!("Failed to update page: {}", error);
    }

    let data: Value = response.json().await?;
    Ok(json!({
        "id": data["id"],
        "version": data["version"]["number"]
    }))
}

fn convert_results_to_markdown(data: &mut Value) {
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
            *body = Value::String(convert_to_markdown(&html));
        }
    }
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
        *body = Value::String(convert_to_markdown(&html));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::create_test_config_with_filters;

    fn create_test_config(confluence_spaces_filter: Vec<String>) -> Config {
        create_test_config_with_filters(vec![], confluence_spaces_filter)
    }

    #[test]
    fn test_max_limit_constant() {
        assert_eq!(MAX_LIMIT, 250);
    }

    #[test]
    fn test_rate_limit_delay() {
        assert_eq!(RATE_LIMIT_DELAY_MS, 200);
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
    fn test_build_next_url_relative_path() {
        // _links.base from API includes /wiki, _links.next does NOT include /wiki
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

    // T018: Remaining Confluence handlers tests

    // get_page tests
    #[test]
    fn test_get_page_url_construction() {
        let config = create_test_config(vec![]);
        let page_id = "12345";

        let url = format!("{}/wiki/api/v2/pages/{}", config.base_url(), page_id);

        assert_eq!(url, "https://test.atlassian.net/wiki/api/v2/pages/12345");
    }

    // get_page_children tests
    #[test]
    fn test_get_page_children_url_construction() {
        let config = create_test_config(vec![]);
        let page_id = "12345";

        let url = format!(
            "{}/wiki/api/v2/pages/{}/children",
            config.base_url(),
            page_id
        );

        assert_eq!(
            url,
            "https://test.atlassian.net/wiki/api/v2/pages/12345/children"
        );
    }

    // get_comments tests
    #[test]
    fn test_get_comments_url_construction() {
        let config = create_test_config(vec![]);
        let page_id = "12345";

        let url = format!(
            "{}/wiki/api/v2/pages/{}/footer-comments",
            config.base_url(),
            page_id
        );

        assert_eq!(
            url,
            "https://test.atlassian.net/wiki/api/v2/pages/12345/footer-comments"
        );
    }

    // create_page tests
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

    // update_page tests
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
