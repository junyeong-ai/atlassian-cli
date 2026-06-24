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

pub fn process_adf_input(value: Value, field_name: &str) -> Result<Value> {
    match value {
        Value::String(text) => {
            // String input is total: a string that is itself a valid ADF
            // document is used as-is; everything else — including JSON-looking
            // prose like `{"status":"done"}` — is the literal body. A string
            // argument therefore never errors, so any text can always be posted.
            let trimmed = text.trim();
            if trimmed.starts_with('{')
                && let Ok(parsed) = serde_json::from_str::<Value>(trimmed)
                && parsed.is_object()
                && validate_adf(&parsed).is_ok()
            {
                return Ok(parsed);
            }
            Ok(text_to_adf(&text))
        }
        Value::Object(_) => {
            validate_adf(&value)?;
            Ok(value)
        }
        Value::Null => Ok(text_to_adf("")),
        _ => {
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
    fn test_process_adf_input_json_string() {
        let adf_json = r#"{"type":"doc","version":1,"content":[{"type":"paragraph","content":[{"type":"text","text":"Hello"}]}]}"#;
        let input = json!(adf_json);
        let result = process_adf_input(input, "test_field").unwrap();

        assert_eq!(result["type"], "doc");
        assert_eq!(result["content"][0]["content"][0]["text"], "Hello");
    }

    #[test]
    fn test_process_adf_input_json_string_with_whitespace() {
        let adf_json = r#"  {"type":"doc","version":1,"content":[]}  "#;
        let input = json!(adf_json);
        let result = process_adf_input(input, "test_field").unwrap();

        assert_eq!(result["type"], "doc");
    }

    #[test]
    fn test_process_adf_input_invalid_json_string() {
        let input = json!("{not valid json}");
        let result = process_adf_input(input, "test_field").unwrap();

        assert_eq!(result["type"], "doc");
        assert_eq!(
            result["content"][0]["content"][0]["text"],
            "{not valid json}"
        );
    }

    #[test]
    fn test_process_adf_input_json_string_not_adf_becomes_literal_text() {
        // A JSON-object-shaped string that is not a valid ADF document is the
        // literal body, not an error — a user must be able to post arbitrary
        // text like `{"foo":"bar"}` as a comment.
        let input = json!(r#"{"foo":"bar"}"#);
        let result = process_adf_input(input, "test_field").unwrap();

        assert_eq!(result["type"], "doc");
        assert_eq!(
            result["content"][0]["content"][0]["text"],
            r#"{"foo":"bar"}"#
        );
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
}
