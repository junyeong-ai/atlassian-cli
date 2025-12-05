use anyhow::Result;
use serde_json::{Value, json};

/// Validates that a Value is a valid ADF (Atlassian Document Format) document.
///
/// A valid ADF document must have:
/// - type: must be exactly "doc"
/// - version: must be integer 1
/// - content: must be an array (can be empty)
///
/// This function performs minimal validation - only checking the top-level document structure.
/// Full content node validation is delegated to the Jira API for performance.
pub fn validate_adf(value: &Value) -> Result<()> {
    // Check if value is an object
    let obj = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Invalid ADF: must be an object"))?;

    // Check required field: type
    let doc_type = obj
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid ADF: missing required field 'type'"))?;

    if doc_type != "doc" {
        anyhow::bail!("Invalid ADF: type must be 'doc', got '{}'", doc_type);
    }

    // Check required field: version
    let version = obj
        .get("version")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow::anyhow!("Invalid ADF: missing required field 'version'"))?;

    if version != 1 {
        anyhow::bail!("Invalid ADF: version must be 1, got {}", version);
    }

    // Check required field: content
    let content = obj
        .get("content")
        .ok_or_else(|| anyhow::anyhow!("Invalid ADF: missing required field 'content'"))?;

    if !content.is_array() {
        anyhow::bail!("Invalid ADF: content must be array");
    }

    Ok(())
}

/// Converts plain text to a simple ADF document with a single paragraph.
///
/// This is the standard conversion for backward compatibility when users
/// provide plain text strings instead of ADF objects.
///
/// # Example
/// ```
/// use serde_json::json;
/// let text = "Hello, world!";
/// // text_to_adf creates simple ADF document
/// let adf = json!({
///     "type": "doc",
///     "version": 1,
///     "content": [{
///         "type": "paragraph",
///         "content": [{"type": "text", "text": text}]
///     }]
/// });
/// assert_eq!(adf["type"], "doc");
/// ```
pub fn text_to_adf(text: &str) -> Value {
    json!({
        "type": "doc",
        "version": 1,
        "content": [{
            "type": "paragraph",
            "content": [{
                "type": "text",
                "text": text
            }]
        }]
    })
}

/// Processes ADF input for any text field (description, comment, etc.)
///
/// This is the core processing function that handles conversion and validation
/// of various input types to ADF format. All field-specific functions delegate
/// to this function.
///
/// Handles three input types:
/// - String: Converts to simple paragraph ADF using text_to_adf
/// - Object: Validates as ADF and returns it (zero-copy via move semantics)
/// - Null: Returns empty paragraph ADF
///
/// # Arguments
/// * `value` - The input value to process (consumed)
/// * `field_name` - Name of the field for error messages (e.g., "description", "comment")
///
/// # Errors
/// Returns error if:
/// - Input is not string, object, or null (e.g., number, boolean, array)
/// - Object fails ADF validation
///
/// # Examples
/// ```
/// use serde_json::{json, Value};
/// use anyhow::Result;
///
/// fn process_adf_input(value: Value, field_name: &str) -> Result<Value> {
///     match value {
///         Value::String(text) => Ok(json!({
///             "type": "doc", "version": 1,
///             "content": [{"type": "paragraph", "content": [{"type": "text", "text": text}]}]
///         })),
///         Value::Object(_) => Ok(value),
///         Value::Null => Ok(json!({"type": "doc", "version": 1, "content": []})),
///         _ => anyhow::bail!("{} must be string or ADF object", field_name)
///     }
/// }
///
/// let adf = process_adf_input(json!("Hello"), "description").unwrap();
/// assert_eq!(adf["type"], "doc");
/// ```
pub fn process_adf_input(value: Value, field_name: &str) -> Result<Value> {
    match value {
        Value::String(text) => {
            // Plain text: convert to simple ADF
            Ok(text_to_adf(&text))
        }
        Value::Object(_) => {
            // ADF object: validate and return (zero-copy via move)
            validate_adf(&value)?;
            Ok(value)
        }
        Value::Null => {
            // Null/missing: return empty paragraph ADF
            Ok(text_to_adf(""))
        }
        _ => {
            // Invalid type (number, boolean, array, etc.)
            anyhow::bail!(
                "{} must be string or ADF object, got {:?}",
                field_name,
                value
            )
        }
    }
}

/// Processes description input for create/update issue operations.
///
/// Convenience wrapper around process_adf_input with field name "description".
/// Consumes the input value for zero-copy processing.
///
/// # Errors
/// Returns error if input is invalid (see process_adf_input for details)
#[inline]
pub fn process_description_input(value: Value) -> Result<Value> {
    process_adf_input(value, "description")
}

/// Processes comment input for add/update comment operations.
///
/// Convenience wrapper around process_adf_input with field name "comment".
/// Consumes the input value for zero-copy processing.
///
/// # Errors
/// Returns error if input is invalid (see process_adf_input for details)
#[inline]
pub fn process_comment_input(value: Value) -> Result<Value> {
    process_adf_input(value, "comment")
}

/// Converts an ADF document to Markdown format.
pub fn adf_to_markdown(adf: &Value) -> String {
    let Some(content) = adf.get("content").and_then(|c| c.as_array()) else {
        return String::new();
    };

    let blocks: Vec<String> = content
        .iter()
        .filter_map(|node| convert_block_node(node, 0))
        .collect();

    normalize_markdown(&blocks.join("\n\n"))
}

fn convert_block_node(node: &Value, depth: usize) -> Option<String> {
    let node_type = node.get("type")?.as_str()?;

    match node_type {
        "paragraph" => convert_paragraph(node),
        "heading" => convert_heading(node),
        "bulletList" => convert_bullet_list(node, depth),
        "orderedList" => convert_ordered_list(node, depth),
        "listItem" => convert_list_item(node, depth),
        "codeBlock" => convert_code_block(node),
        "blockquote" => convert_blockquote(node),
        "rule" => Some("---".to_string()),
        "panel" => convert_panel(node),
        "table" => convert_table(node),
        "mediaSingle" | "mediaGroup" => convert_media(node),
        "expand" => convert_expand(node),
        _ => convert_paragraph(node),
    }
}

fn convert_paragraph(node: &Value) -> Option<String> {
    let content = node.get("content")?.as_array()?;
    let text = convert_inline_nodes(content);
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

fn convert_heading(node: &Value) -> Option<String> {
    let level = node
        .get("attrs")
        .and_then(|a| a.get("level"))
        .and_then(|l| l.as_u64())
        .unwrap_or(1) as usize;

    let content = node.get("content").and_then(|c| c.as_array())?;
    let text = convert_inline_nodes(content);

    if text.trim().is_empty() {
        None
    } else {
        Some(format!("{} {}", "#".repeat(level.min(6)), text))
    }
}

fn convert_bullet_list(node: &Value, depth: usize) -> Option<String> {
    let items = node.get("content")?.as_array()?;
    let lines: Vec<String> = items
        .iter()
        .filter_map(|item| {
            let content = convert_list_item(item, depth)?;
            Some(format!("{}- {}", "  ".repeat(depth), content))
        })
        .collect();

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn convert_ordered_list(node: &Value, depth: usize) -> Option<String> {
    let items = node.get("content")?.as_array()?;
    let lines: Vec<String> = items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| {
            let content = convert_list_item(item, depth)?;
            Some(format!("{}{}. {}", "  ".repeat(depth), i + 1, content))
        })
        .collect();

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn convert_list_item(node: &Value, depth: usize) -> Option<String> {
    let content = node.get("content")?.as_array()?;
    let mut parts: Vec<String> = Vec::new();

    for child in content {
        let child_type = child.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match child_type {
            "paragraph" => {
                if let Some(text) = convert_paragraph(child) {
                    parts.push(text);
                }
            }
            "bulletList" => {
                if let Some(list) = convert_bullet_list(child, depth + 1) {
                    parts.push(format!("\n{}", list));
                }
            }
            "orderedList" => {
                if let Some(list) = convert_ordered_list(child, depth + 1) {
                    parts.push(format!("\n{}", list));
                }
            }
            _ => {
                if let Some(text) = convert_block_node(child, depth + 1) {
                    parts.push(text);
                }
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

fn convert_code_block(node: &Value) -> Option<String> {
    let language = node
        .get("attrs")
        .and_then(|a| a.get("language"))
        .and_then(|l| l.as_str())
        .unwrap_or("");

    let content = node.get("content").and_then(|c| c.as_array());
    let code = content
        .map(|nodes| {
            nodes
                .iter()
                .filter_map(|n| n.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();

    Some(format!("```{}\n{}\n```", language, code))
}

fn convert_blockquote(node: &Value) -> Option<String> {
    let content = node.get("content")?.as_array()?;
    let lines: Vec<String> = content
        .iter()
        .filter_map(|child| convert_block_node(child, 0))
        .flat_map(|text| text.lines().map(|l| format!("> {}", l)).collect::<Vec<_>>())
        .collect();

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn convert_panel(node: &Value) -> Option<String> {
    let panel_type = node
        .get("attrs")
        .and_then(|a| a.get("panelType"))
        .and_then(|p| p.as_str())
        .unwrap_or("info")
        .to_uppercase();

    let content = node.get("content")?.as_array()?;
    let text: Vec<String> = content
        .iter()
        .filter_map(|child| convert_block_node(child, 0))
        .collect();

    if text.is_empty() {
        None
    } else {
        Some(format!("> **{}**: {}", panel_type, text.join(" ")))
    }
}

fn convert_table(node: &Value) -> Option<String> {
    let rows = node.get("content")?.as_array()?;
    if rows.is_empty() {
        return None;
    }

    let mut result: Vec<String> = Vec::new();
    let mut is_header = true;

    for row in rows {
        let cells = row.get("content").and_then(|c| c.as_array())?;
        let cell_texts: Vec<String> = cells
            .iter()
            .map(|cell| {
                cell.get("content")
                    .and_then(|c| c.as_array())
                    .map(|content| {
                        content
                            .iter()
                            .filter_map(|n| convert_block_node(n, 0))
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default()
            })
            .collect();

        result.push(format!("| {} |", cell_texts.join(" | ")));

        if is_header {
            let separator: Vec<&str> = vec!["---"; cell_texts.len()];
            result.push(format!("| {} |", separator.join(" | ")));
            is_header = false;
        }
    }

    Some(result.join("\n"))
}

fn convert_media(node: &Value) -> Option<String> {
    let content = node.get("content").and_then(|c| c.as_array())?;
    for media in content {
        if let Some(attrs) = media.get("attrs") {
            let alt = attrs
                .get("alt")
                .and_then(|a| a.as_str())
                .or_else(|| attrs.get("id").and_then(|i| i.as_str()))
                .unwrap_or("media");
            return Some(format!("[Media: {}]", alt));
        }
    }
    Some("[Media]".to_string())
}

fn convert_expand(node: &Value) -> Option<String> {
    let title = node
        .get("attrs")
        .and_then(|a| a.get("title"))
        .and_then(|t| t.as_str())
        .unwrap_or("Details");

    let content = node.get("content")?.as_array()?;
    let text: Vec<String> = content
        .iter()
        .filter_map(|child| convert_block_node(child, 0))
        .collect();

    if text.is_empty() {
        None
    } else {
        Some(format!("**{}**\n\n{}", title, text.join("\n\n")))
    }
}

fn convert_inline_nodes(nodes: &[Value]) -> String {
    nodes.iter().map(convert_inline_node).collect()
}

fn convert_inline_node(node: &Value) -> String {
    let node_type = node.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match node_type {
        "text" => convert_text_node(node),
        "hardBreak" => "\n".to_string(),
        "mention" => convert_mention(node),
        "emoji" => convert_emoji(node),
        "inlineCard" => convert_inline_card(node),
        _ => String::new(),
    }
}

fn convert_text_node(node: &Value) -> String {
    let text = node
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();

    let Some(marks) = node.get("marks").and_then(|m| m.as_array()) else {
        return text;
    };

    let mut result = text;
    for mark in marks {
        let mark_type = mark.get("type").and_then(|t| t.as_str()).unwrap_or("");
        result = match mark_type {
            "strong" => format!("**{}**", result),
            "em" => format!("*{}*", result),
            "code" => format!("`{}`", result),
            "strike" => format!("~~{}~~", result),
            "link" => {
                let href = mark
                    .get("attrs")
                    .and_then(|a| a.get("href"))
                    .and_then(|h| h.as_str())
                    .unwrap_or("");
                format!("[{}]({})", result, href)
            }
            _ => result,
        };
    }

    result
}

fn convert_mention(node: &Value) -> String {
    let attrs = node.get("attrs");
    let text = attrs
        .and_then(|a| a.get("text"))
        .and_then(|t| t.as_str())
        .or_else(|| attrs.and_then(|a| a.get("id")).and_then(|i| i.as_str()))
        .unwrap_or("user");

    format!("@{}", text.trim_start_matches('@'))
}

fn convert_emoji(node: &Value) -> String {
    node.get("attrs")
        .and_then(|a| a.get("shortName"))
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

fn convert_inline_card(node: &Value) -> String {
    let url = node
        .get("attrs")
        .and_then(|a| a.get("url"))
        .and_then(|u| u.as_str())
        .unwrap_or("");

    if url.is_empty() {
        String::new()
    } else {
        format!("[{}]({})", url, url)
    }
}

fn normalize_markdown(text: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    let mut prev_empty = false;

    for line in text.lines() {
        let is_empty = line.trim().is_empty();
        if is_empty {
            if !prev_empty {
                lines.push("");
            }
            prev_empty = true;
        } else {
            lines.push(line);
            prev_empty = false;
        }
    }

    while lines.first() == Some(&"") {
        lines.remove(0);
    }
    while lines.last() == Some(&"") {
        lines.pop();
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests for validate_adf function

    #[test]
    fn test_validate_adf_valid_document() {
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": []
        });

        assert!(validate_adf(&adf).is_ok());
    }

    #[test]
    fn test_validate_adf_valid_document_with_content() {
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "paragraph",
                "content": [{"type": "text", "text": "Hello"}]
            }]
        });

        assert!(validate_adf(&adf).is_ok());
    }

    #[test]
    fn test_validate_adf_missing_type() {
        let adf = json!({
            "version": 1,
            "content": []
        });

        let result = validate_adf(&adf);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("missing required field 'type'")
        );
    }

    #[test]
    fn test_validate_adf_wrong_type() {
        let adf = json!({
            "type": "paragraph",
            "version": 1,
            "content": []
        });

        let result = validate_adf(&adf);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("type must be 'doc'")
        );
    }

    #[test]
    fn test_validate_adf_wrong_version() {
        let adf = json!({
            "type": "doc",
            "version": 2,
            "content": []
        });

        let result = validate_adf(&adf);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("version must be 1")
        );
    }

    #[test]
    fn test_validate_adf_missing_version() {
        let adf = json!({
            "type": "doc",
            "content": []
        });

        let result = validate_adf(&adf);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("missing required field 'version'")
        );
    }

    #[test]
    fn test_validate_adf_invalid_content() {
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": "invalid"
        });

        let result = validate_adf(&adf);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("content must be array")
        );
    }

    #[test]
    fn test_validate_adf_missing_content() {
        let adf = json!({
            "type": "doc",
            "version": 1
        });

        let result = validate_adf(&adf);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("missing required field 'content'")
        );
    }

    #[test]
    fn test_validate_adf_not_object() {
        let adf = json!("not an object");

        let result = validate_adf(&adf);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must be an object")
        );
    }

    // Tests for text_to_adf function

    #[test]
    fn test_text_to_adf_simple() {
        let text = "Hello, world!";
        let adf = text_to_adf(text);

        assert_eq!(adf["type"], "doc");
        assert_eq!(adf["version"], 1);
        assert_eq!(adf["content"][0]["type"], "paragraph");
        assert_eq!(adf["content"][0]["content"][0]["type"], "text");
        assert_eq!(adf["content"][0]["content"][0]["text"], "Hello, world!");
    }

    #[test]
    fn test_text_to_adf_empty() {
        let adf = text_to_adf("");

        assert_eq!(adf["type"], "doc");
        assert_eq!(adf["version"], 1);
        assert_eq!(adf["content"][0]["content"][0]["text"], "");
    }

    #[test]
    fn test_text_to_adf_special_characters() {
        let text = "Test with \"quotes\" and 'apostrophes' and <brackets>";
        let adf = text_to_adf(text);

        assert_eq!(adf["content"][0]["content"][0]["text"], text);
    }

    #[test]
    fn test_text_to_adf_newlines() {
        let text = "Line 1\nLine 2\nLine 3";
        let adf = text_to_adf(text);

        assert_eq!(adf["content"][0]["content"][0]["text"], text);
    }

    #[test]
    fn test_text_to_adf_unicode() {
        let text = "Unicode: 你好 🎉 café";
        let adf = text_to_adf(text);

        assert_eq!(adf["content"][0]["content"][0]["text"], text);
    }

    // Tests for process_adf_input function (core logic)

    #[test]
    fn test_process_adf_input_string() {
        let input = json!("Plain text");
        let result = process_adf_input(input, "test_field").unwrap();

        assert_eq!(result["type"], "doc");
        assert_eq!(result["version"], 1);
        assert_eq!(result["content"][0]["content"][0]["text"], "Plain text");
    }

    #[test]
    fn test_process_adf_input_empty_string() {
        let input = json!("");
        let result = process_adf_input(input, "test_field").unwrap();

        assert_eq!(result["type"], "doc");
        assert_eq!(result["content"][0]["content"][0]["text"], "");
    }

    #[test]
    fn test_process_adf_input_valid_adf_object() {
        let input = json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "heading",
                "attrs": {"level": 2},
                "content": [{"type": "text", "text": "Title"}]
            }]
        });

        let result = process_adf_input(input, "test_field").unwrap();
        assert_eq!(result["type"], "doc");
        assert_eq!(result["content"][0]["type"], "heading");
        assert_eq!(result["content"][0]["attrs"]["level"], 2);
    }

    #[test]
    fn test_process_adf_input_complex_adf() {
        let input = json!({
            "type": "doc",
            "version": 1,
            "content": [
                {
                    "type": "heading",
                    "attrs": {"level": 2},
                    "content": [{"type": "text", "text": "Problem"}]
                },
                {
                    "type": "paragraph",
                    "content": [
                        {"type": "text", "text": "The "},
                        {"type": "text", "text": "API", "marks": [{"type": "code"}]},
                        {"type": "text", "text": " is broken"}
                    ]
                },
                {
                    "type": "codeBlock",
                    "attrs": {"language": "rust"},
                    "content": [{"type": "text", "text": "fn main() {}"}]
                }
            ]
        });

        let result = process_adf_input(input, "test_field").unwrap();
        assert_eq!(result["type"], "doc");
        assert_eq!(result["content"].as_array().unwrap().len(), 3);
        assert_eq!(result["content"][0]["type"], "heading");
        assert_eq!(result["content"][1]["type"], "paragraph");
        assert_eq!(result["content"][2]["type"], "codeBlock");
    }

    #[test]
    fn test_process_adf_input_null() {
        let input = json!(null);
        let result = process_adf_input(input, "test_field").unwrap();

        assert_eq!(result["type"], "doc");
        assert_eq!(result["content"][0]["content"][0]["text"], "");
    }

    #[test]
    fn test_process_adf_input_invalid_type_number() {
        let input = json!(123);
        let result = process_adf_input(input, "test_field");

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("test_field must be string or ADF object"));
    }

    #[test]
    fn test_process_adf_input_invalid_type_boolean() {
        let input = json!(true);
        let result = process_adf_input(input, "my_field");

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("my_field must be string or ADF object"));
    }

    #[test]
    fn test_process_adf_input_invalid_type_array() {
        let input = json!(["not", "an", "adf"]);
        let result = process_adf_input(input, "custom_field");

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("custom_field must be string or ADF object"));
    }

    #[test]
    fn test_process_adf_input_invalid_adf_object() {
        let input = json!({
            "type": "paragraph",  // Wrong type
            "version": 1,
            "content": []
        });

        let result = process_adf_input(input, "test_field");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("type must be 'doc'")
        );
    }

    #[test]
    fn test_process_adf_input_field_name_in_error() {
        let input = json!(123);
        let result = process_adf_input(input, "my_custom_field");

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("my_custom_field"),
            "Error message should include field name, got: {}",
            error_msg
        );
    }

    // Tests for wrapper functions

    #[test]
    fn test_process_description_input_delegates_correctly() {
        let input = json!("Test description");
        let result = process_description_input(input).unwrap();

        assert_eq!(result["type"], "doc");
        assert_eq!(
            result["content"][0]["content"][0]["text"],
            "Test description"
        );
    }

    #[test]
    fn test_process_description_input_error_includes_field_name() {
        let input = json!(123);
        let result = process_description_input(input);

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("description"),
            "Error message should include 'description', got: {}",
            error_msg
        );
    }

    #[test]
    fn test_process_comment_input_delegates_correctly() {
        let input = json!("Test comment");
        let result = process_comment_input(input).unwrap();

        assert_eq!(result["type"], "doc");
        assert_eq!(result["content"][0]["content"][0]["text"], "Test comment");
    }

    #[test]
    fn test_process_comment_input_error_includes_field_name() {
        let input = json!(true);
        let result = process_comment_input(input);

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("comment"),
            "Error message should include 'comment', got: {}",
            error_msg
        );
    }

    // Performance test

    #[test]
    fn test_adf_validation_performance() {
        use std::time::Instant;

        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "paragraph",
                "content": [{"type": "text", "text": "test"}]
            }]
        });

        let start = Instant::now();
        for _ in 0..1000 {
            validate_adf(&adf).unwrap();
        }
        let duration = start.elapsed();

        let avg_ms = duration.as_micros() as f64 / 1000.0 / 1000.0;
        println!("Average validation time: {:.3}ms", avg_ms);

        // Should be < 10ms average (NFR3 requirement)
        assert!(avg_ms < 10.0, "Validation too slow: {}ms > 10ms", avg_ms);
    }

    // Edge case tests: version field type validation

    #[test]
    fn test_validate_adf_version_as_float() {
        // Test that version 1.0 (float) is rejected - must be integer
        let adf = json!({
            "type": "doc",
            "version": 1.0,
            "content": []
        });

        let result = validate_adf(&adf);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("missing required field 'version'"),
            "Float version should be rejected (as_i64 fails)"
        );
    }

    #[test]
    fn test_validate_adf_version_as_string() {
        // Test that version "1" (string) is rejected
        let adf = json!({
            "type": "doc",
            "version": "1",
            "content": []
        });

        let result = validate_adf(&adf);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("missing required field 'version'")
        );
    }

    #[test]
    fn test_validate_adf_version_zero() {
        // Test that version 0 is rejected
        let adf = json!({
            "type": "doc",
            "version": 0,
            "content": []
        });

        let result = validate_adf(&adf);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("version must be 1")
        );
    }

    #[test]
    fn test_validate_adf_version_negative() {
        // Test that negative version is rejected
        let adf = json!({
            "type": "doc",
            "version": -1,
            "content": []
        });

        let result = validate_adf(&adf);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("version must be 1")
        );
    }

    // Edge case tests: large text handling

    #[test]
    fn test_text_to_adf_large_text() {
        // Test 50KB text conversion (typical large Jira description)
        let large_text = "x".repeat(50_000);
        let adf = text_to_adf(&large_text);

        assert_eq!(adf["type"], "doc");
        assert_eq!(adf["version"], 1);
        assert_eq!(adf["content"][0]["content"][0]["text"], large_text);
        assert_eq!(
            adf["content"][0]["content"][0]["text"]
                .as_str()
                .unwrap()
                .len(),
            50_000
        );
    }

    #[test]
    fn test_process_adf_input_large_string() {
        // Test processing 100KB string doesn't panic
        let large_text = "Large description text. ".repeat(4_000); // ~100KB
        let input = json!(large_text.clone());
        let result = process_adf_input(input, "description").unwrap();

        assert_eq!(result["type"], "doc");
        assert_eq!(result["content"][0]["content"][0]["text"], large_text);
    }

    #[test]
    fn test_text_to_adf_performance_large_text() {
        use std::time::Instant;

        // Test that 10KB text conversion is fast (< 1ms)
        let text = "x".repeat(10_000);

        let start = Instant::now();
        for _ in 0..100 {
            text_to_adf(&text);
        }
        let duration = start.elapsed();

        let avg_ms = duration.as_micros() as f64 / 100.0 / 1000.0;
        println!("Average 10KB text conversion time: {:.3}ms", avg_ms);

        // Should be very fast (< 1ms per conversion)
        assert!(avg_ms < 1.0, "Text conversion too slow: {}ms > 1ms", avg_ms);
    }

    // Edge case tests: deeply nested structures

    #[test]
    fn test_validate_adf_deeply_nested_content() {
        // Test that deeply nested content passes validation
        // (we only validate top-level, so deep nesting is OK)
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "bulletList",
                "content": [{
                    "type": "listItem",
                    "content": [{
                        "type": "paragraph",
                        "content": [{
                            "type": "text",
                            "text": "Nested",
                            "marks": [
                                {"type": "strong"},
                                {"type": "em"},
                                {"type": "code"}
                            ]
                        }]
                    }]
                }]
            }]
        });

        let result = validate_adf(&adf);
        assert!(result.is_ok(), "Deeply nested structure should be valid");
    }

    #[test]
    fn test_process_adf_input_complex_nested_lists() {
        // Test mixed list types with deep nesting
        let input = json!({
            "type": "doc",
            "version": 1,
            "content": [
                {
                    "type": "bulletList",
                    "content": [{
                        "type": "listItem",
                        "content": [{
                            "type": "paragraph",
                            "content": [{"type": "text", "text": "Bullet 1"}]
                        }, {
                            "type": "orderedList",
                            "content": [{
                                "type": "listItem",
                                "content": [{
                                    "type": "paragraph",
                                    "content": [{"type": "text", "text": "Nested ordered"}]
                                }]
                            }]
                        }]
                    }]
                },
                {
                    "type": "orderedList",
                    "content": [{
                        "type": "listItem",
                        "content": [{
                            "type": "paragraph",
                            "content": [{"type": "text", "text": "Ordered 1"}]
                        }]
                    }]
                }
            ]
        });

        let result = process_adf_input(input, "description").unwrap();
        assert_eq!(result["type"], "doc");
        assert_eq!(result["content"].as_array().unwrap().len(), 2);
        assert_eq!(result["content"][0]["type"], "bulletList");
        assert_eq!(result["content"][1]["type"], "orderedList");
    }

    // Edge case tests: multiple marks and formatting

    #[test]
    fn test_process_adf_input_multiple_text_marks() {
        // Test text with multiple marks (bold + italic + code)
        let input = json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "paragraph",
                "content": [{
                    "type": "text",
                    "text": "Important API",
                    "marks": [
                        {"type": "strong"},
                        {"type": "em"},
                        {"type": "code"}
                    ]
                }]
            }]
        });

        let result = process_adf_input(input, "description").unwrap();
        assert_eq!(result["type"], "doc");
        assert_eq!(
            result["content"][0]["content"][0]["marks"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn test_validate_adf_mixed_content_types() {
        // Test document with all major block types
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": [
                {
                    "type": "heading",
                    "attrs": {"level": 1},
                    "content": [{"type": "text", "text": "Title"}]
                },
                {
                    "type": "paragraph",
                    "content": [{"type": "text", "text": "Normal text"}]
                },
                {
                    "type": "bulletList",
                    "content": [{
                        "type": "listItem",
                        "content": [{
                            "type": "paragraph",
                            "content": [{"type": "text", "text": "Item"}]
                        }]
                    }]
                },
                {
                    "type": "orderedList",
                    "content": [{
                        "type": "listItem",
                        "content": [{
                            "type": "paragraph",
                            "content": [{"type": "text", "text": "Step 1"}]
                        }]
                    }]
                },
                {
                    "type": "codeBlock",
                    "attrs": {"language": "rust"},
                    "content": [{"type": "text", "text": "fn main() {}"}]
                },
                {
                    "type": "panel",
                    "attrs": {"panelType": "info"},
                    "content": [{
                        "type": "paragraph",
                        "content": [{"type": "text", "text": "Info panel"}]
                    }]
                }
            ]
        });

        let result = validate_adf(&adf);
        assert!(result.is_ok(), "Mixed content types should be valid");
    }

    // Edge case tests: empty and whitespace

    #[test]
    fn test_text_to_adf_only_whitespace() {
        let text = "   \n\t\r\n   ";
        let adf = text_to_adf(text);

        assert_eq!(adf["content"][0]["content"][0]["text"], text);
    }

    #[test]
    fn test_process_adf_input_empty_content_array() {
        // Empty content array is valid
        let input = json!({
            "type": "doc",
            "version": 1,
            "content": []
        });

        let result = process_adf_input(input, "description").unwrap();
        assert_eq!(result["type"], "doc");
        assert_eq!(result["content"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_validate_adf_content_with_many_elements() {
        // Test document with many top-level elements (100+)
        let mut content = Vec::new();
        for i in 0..150 {
            content.push(json!({
                "type": "paragraph",
                "content": [{"type": "text", "text": format!("Paragraph {}", i)}]
            }));
        }

        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": content
        });

        let result = validate_adf(&adf);
        assert!(
            result.is_ok(),
            "Document with 150 paragraphs should be valid"
        );
    }

    // Tests for adf_to_markdown function

    #[test]
    fn test_adf_to_markdown_simple_paragraph() {
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "paragraph",
                "content": [{"type": "text", "text": "Hello world"}]
            }]
        });

        let result = adf_to_markdown(&adf);
        assert_eq!(result, "Hello world");
    }

    #[test]
    fn test_adf_to_markdown_heading() {
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "heading",
                "attrs": {"level": 2},
                "content": [{"type": "text", "text": "Title"}]
            }]
        });

        let result = adf_to_markdown(&adf);
        assert_eq!(result, "## Title");
    }

    #[test]
    fn test_adf_to_markdown_bold_italic() {
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "paragraph",
                "content": [
                    {"type": "text", "text": "bold", "marks": [{"type": "strong"}]},
                    {"type": "text", "text": " and "},
                    {"type": "text", "text": "italic", "marks": [{"type": "em"}]}
                ]
            }]
        });

        let result = adf_to_markdown(&adf);
        assert_eq!(result, "**bold** and *italic*");
    }

    #[test]
    fn test_adf_to_markdown_link() {
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "paragraph",
                "content": [{
                    "type": "text",
                    "text": "click here",
                    "marks": [{"type": "link", "attrs": {"href": "https://example.com"}}]
                }]
            }]
        });

        let result = adf_to_markdown(&adf);
        assert_eq!(result, "[click here](https://example.com)");
    }

    #[test]
    fn test_adf_to_markdown_bullet_list() {
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "bulletList",
                "content": [
                    {"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Item 1"}]}]},
                    {"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Item 2"}]}]}
                ]
            }]
        });

        let result = adf_to_markdown(&adf);
        assert!(result.contains("- Item 1"));
        assert!(result.contains("- Item 2"));
    }

    #[test]
    fn test_adf_to_markdown_ordered_list() {
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "orderedList",
                "content": [
                    {"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "First"}]}]},
                    {"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Second"}]}]}
                ]
            }]
        });

        let result = adf_to_markdown(&adf);
        assert!(result.contains("1. First"));
        assert!(result.contains("2. Second"));
    }

    #[test]
    fn test_adf_to_markdown_code_block() {
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "codeBlock",
                "attrs": {"language": "rust"},
                "content": [{"type": "text", "text": "fn main() {}"}]
            }]
        });

        let result = adf_to_markdown(&adf);
        assert!(result.contains("```rust"));
        assert!(result.contains("fn main() {}"));
        assert!(result.contains("```"));
    }

    #[test]
    fn test_adf_to_markdown_inline_code() {
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "paragraph",
                "content": [
                    {"type": "text", "text": "Use "},
                    {"type": "text", "text": "code", "marks": [{"type": "code"}]},
                    {"type": "text", "text": " here"}
                ]
            }]
        });

        let result = adf_to_markdown(&adf);
        assert_eq!(result, "Use `code` here");
    }

    #[test]
    fn test_adf_to_markdown_panel() {
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "panel",
                "attrs": {"panelType": "info"},
                "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Important note"}]}]
            }]
        });

        let result = adf_to_markdown(&adf);
        assert!(result.contains("**INFO**"));
        assert!(result.contains("Important note"));
    }

    #[test]
    fn test_adf_to_markdown_mention() {
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "paragraph",
                "content": [
                    {"type": "text", "text": "Hello "},
                    {"type": "mention", "attrs": {"text": "@john"}}
                ]
            }]
        });

        let result = adf_to_markdown(&adf);
        assert!(result.contains("@john"));
    }

    #[test]
    fn test_adf_to_markdown_table() {
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "table",
                "content": [
                    {"type": "tableRow", "content": [
                        {"type": "tableHeader", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "A"}]}]},
                        {"type": "tableHeader", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "B"}]}]}
                    ]},
                    {"type": "tableRow", "content": [
                        {"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "1"}]}]},
                        {"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "2"}]}]}
                    ]}
                ]
            }]
        });

        let result = adf_to_markdown(&adf);
        assert!(result.contains("| A | B |"));
        assert!(result.contains("| --- | --- |"));
        assert!(result.contains("| 1 | 2 |"));
    }

    #[test]
    fn test_adf_to_markdown_empty_content() {
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": []
        });

        let result = adf_to_markdown(&adf);
        assert!(result.is_empty());
    }

    #[test]
    fn test_adf_to_markdown_rule() {
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "Above"}]},
                {"type": "rule"},
                {"type": "paragraph", "content": [{"type": "text", "text": "Below"}]}
            ]
        });

        let result = adf_to_markdown(&adf);
        assert!(result.contains("---"));
    }
}
