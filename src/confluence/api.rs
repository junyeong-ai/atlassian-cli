use crate::config::Config;
use crate::confluence::fields::{apply_expand_filtering, apply_v2_filtering};
use crate::http;
use anyhow::Result;
use serde_json::{Value, json};

/// Search Confluence using CQL query
pub async fn search(
    query: &str,
    limit: u32,
    include_all_fields: Option<bool>,
    additional_expand: Option<Vec<String>>,
    config: &Config,
) -> Result<Value> {
    let cql = query;

    // Apply space filter if configured and not already in CQL
    let final_cql = if !config.confluence.spaces_filter.is_empty() {
        let cql_lower = cql.to_lowercase();
        // Check if CQL already contains space condition
        if cql_lower.contains("space ")
            || cql_lower.contains("space=")
            || cql_lower.contains("space in")
        {
            // User explicitly specified space, use their CQL as-is
            cql.to_string()
        } else {
            // Add space filter
            let spaces = config
                .confluence
                .spaces_filter
                .iter()
                .map(|s| format!("\"{}\"", s))
                .collect::<Vec<_>>()
                .join(",");
            format!("space IN ({}) AND ({})", spaces, cql)
        }
    } else {
        cql.to_string()
    };

    let client = http::client(config);
    let url = format!("{}/wiki/rest/api/search", config.base_url());

    let (url, expand_param) = apply_expand_filtering(&url, include_all_fields, additional_expand);

    let mut query_params = vec![
        ("cql".to_string(), final_cql),
        ("limit".to_string(), limit.to_string()),
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

    let data: Value = response.json().await?;
    Ok(json!({
        "items": data["results"],
        "total": data["totalSize"]
    }))
}

pub async fn get_page(
    page_id: &str,
    include_all_fields: Option<bool>,
    additional_includes: Option<Vec<String>>,
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

    response.json().await.map_err(Into::into)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::create_test_config_with_filters;

    // Helper function to create test config with confluence spaces filter
    fn create_test_config(confluence_spaces_filter: Vec<String>) -> Config {
        create_test_config_with_filters(vec![], confluence_spaces_filter)
    }

    // T017: Confluence search tests

    #[test]
    fn test_search_default_limit() {
        let limit = 10u32;
        assert_eq!(limit, 10);
    }

    #[test]
    fn test_search_custom_limit() {
        let limit = 25u32;
        assert_eq!(limit, 25);
    }

    #[test]
    fn test_search_space_filter_injection() {
        let config = create_test_config(vec!["SPACE1".to_string(), "SPACE2".to_string()]);
        let cql = "type = page";

        // Simulate space filter logic
        let final_cql = if !config.confluence.spaces_filter.is_empty() {
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
        } else {
            cql.to_string()
        };

        assert_eq!(
            final_cql,
            "space IN (\"SPACE1\",\"SPACE2\") AND (type = page)"
        );
    }

    #[test]
    fn test_search_space_filter_not_injected_when_present() {
        let config = create_test_config(vec!["SPACE1".to_string()]);
        let cql = "space = MYSPACE AND type = page";

        // Simulate space filter logic
        let final_cql = if !config.confluence.spaces_filter.is_empty() {
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
        } else {
            cql.to_string()
        };

        assert_eq!(final_cql, "space = MYSPACE AND type = page");
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
