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

/// Wraps text, verbatim, in a one-paragraph ADF document. Empty text yields an
/// empty paragraph, because ADF rejects an empty text node.
pub fn text_to_adf(text: &str) -> Value {
    let content = if text.is_empty() {
        Vec::new()
    } else {
        vec![json!({ "type": "text", "text": text })]
    };
    json!({
        "type": "doc",
        "version": 1,
        "content": [{
            "type": "paragraph",
            "content": content
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

pub fn process_description_input(value: Value) -> Result<Value> {
    process_adf_input(value, "description")
}

pub fn process_comment_input(value: Value) -> Result<Value> {
    process_adf_input(value, "comment")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(content: Value) -> Value {
        json!({ "type": "doc", "version": 1, "content": content })
    }

    #[test]
    fn validation_checks_only_the_document_root() {
        let nested_is_jira_s_to_judge = doc(json!([{ "type": "unknownNode" }]));
        assert!(validate_adf(&doc(json!([]))).is_ok());
        assert!(validate_adf(&nested_is_jira_s_to_judge).is_ok());

        for (invalid, expected) in [
            (json!("doc"), "must be an object"),
            (
                json!({ "version": 1, "content": [] }),
                "missing required field 'type'",
            ),
            (
                json!({ "type": "paragraph", "version": 1, "content": [] }),
                "type must be 'doc'",
            ),
            (
                json!({ "type": "doc", "content": [] }),
                "missing required field 'version'",
            ),
            (
                json!({ "type": "doc", "version": "1", "content": [] }),
                "missing required field 'version'",
            ),
            (
                json!({ "type": "doc", "version": 1.0, "content": [] }),
                "missing required field 'version'",
            ),
            (
                json!({ "type": "doc", "version": 2, "content": [] }),
                "version must be 1",
            ),
            (
                json!({ "type": "doc", "version": 1 }),
                "missing required field 'content'",
            ),
            (
                json!({ "type": "doc", "version": 1, "content": {} }),
                "content must be array",
            ),
        ] {
            let err = validate_adf(&invalid).unwrap_err().to_string();
            assert!(err.contains(expected), "{invalid}: {err}");
        }
    }

    #[test]
    fn text_is_wrapped_verbatim_and_empty_text_has_no_text_node() {
        let text = "  <b>x</b> & \"y\"\nline 2 — 한글 \u{1F600}  ";
        assert_eq!(
            text_to_adf(text),
            doc(json!([{ "type": "paragraph", "content": [{ "type": "text", "text": text }] }]))
        );
        assert_eq!(
            text_to_adf(""),
            doc(json!([{ "type": "paragraph", "content": [] }]))
        );
    }

    #[test]
    fn a_string_is_an_adf_document_only_when_it_parses_as_one() {
        let document = doc(json!([{ "type": "paragraph", "content": [] }]));
        assert_eq!(
            process_adf_input(Value::String(format!("  {document}\n")), "description").unwrap(),
            document
        );
        for literal in [r#"{"status":"done"}"#, r#"{"type": "doc""#, "plain"] {
            assert_eq!(
                process_adf_input(Value::String(literal.into()), "description").unwrap(),
                text_to_adf(literal),
                "{literal}"
            );
        }
    }

    #[test]
    fn non_string_input_must_be_a_valid_document_or_null() {
        let document = doc(json!([]));
        assert_eq!(process_comment_input(document.clone()).unwrap(), document);
        assert_eq!(process_comment_input(Value::Null).unwrap(), text_to_adf(""));

        let err = process_description_input(json!({ "type": "doc" })).unwrap_err();
        assert!(err.to_string().contains("Invalid ADF"), "{err}");
        for invalid in [json!(42), json!(true), json!(["a"])] {
            let err = process_comment_input(invalid.clone())
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("comment must be string or ADF object"),
                "{invalid}: {err}"
            );
        }
    }
}
