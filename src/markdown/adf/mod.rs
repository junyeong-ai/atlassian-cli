mod blocks;
mod from_markdown;
mod inline;
mod marks;

pub use from_markdown::markdown_to_adf;

/// How deeply blocks may nest, in both directions: the reader truncates past
/// it, and the writer refuses to produce what the reader would truncate.
const MAX_DEPTH: usize = 50;

use crate::markdown::common::normalize_whitespace;
use serde_json::Value;

/// Convert ADF to Markdown, replacing excessively nested blocks with a visible
/// truncation marker while continuing with subsequent sibling blocks.
pub fn adf_to_markdown(adf: &Value) -> String {
    let Some(content) = adf.get("content").and_then(|c| c.as_array()) else {
        return String::new();
    };

    let blocks: Vec<String> = content
        .iter()
        .filter_map(|node| blocks::convert_block_node(node, 0, 0))
        .collect();

    normalize_whitespace(&blocks.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_empty_doc() {
        let adf = json!({"type": "doc", "version": 1, "content": []});
        assert_eq!(adf_to_markdown(&adf), "");
    }

    #[test]
    fn test_simple_paragraph() {
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Hello"}]}]
        });
        assert_eq!(adf_to_markdown(&adf), "Hello");
    }

    #[test]
    fn test_multiple_paragraphs() {
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "First"}]},
                {"type": "paragraph", "content": [{"type": "text", "text": "Second"}]}
            ]
        });
        assert_eq!(adf_to_markdown(&adf), "First\n\nSecond");
    }

    #[test]
    fn test_formatted_text() {
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
        assert_eq!(adf_to_markdown(&adf), "**bold** and *italic*");
    }

    #[test]
    fn test_heading_and_paragraph() {
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": [
                {"type": "heading", "attrs": {"level": 1}, "content": [{"type": "text", "text": "Title"}]},
                {"type": "paragraph", "content": [{"type": "text", "text": "Content"}]}
            ]
        });
        assert_eq!(adf_to_markdown(&adf), "# Title\n\nContent");
    }

    #[test]
    fn test_code_block() {
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "codeBlock",
                "attrs": {"language": "rust"},
                "content": [{"type": "text", "text": "let x = 1;"}]
            }]
        });
        let result = adf_to_markdown(&adf);
        assert!(result.contains("```rust"));
        assert!(result.contains("let x = 1;"));
    }

    #[test]
    fn test_link() {
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
        assert_eq!(adf_to_markdown(&adf), "[click here](https://example.com)");
    }

    #[test]
    fn test_table() {
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "table",
                "content": [
                    {"type": "tableRow", "content": [
                        {"type": "tableHeader", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Header"}]}]}
                    ]},
                    {"type": "tableRow", "content": [
                        {"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Cell"}]}]}
                    ]}
                ]
            }]
        });
        let result = adf_to_markdown(&adf);
        assert!(result.contains("| Header |"));
        assert!(result.contains("| --- |"));
        assert!(result.contains("| Cell |"));
    }

    fn block(kind: &str, content: Vec<Value>) -> Value {
        Value::Object(serde_json::Map::from_iter([
            ("type".into(), Value::String(kind.into())),
            ("content".into(), Value::Array(content)),
        ]))
    }

    fn paragraph(text: &str) -> Value {
        json!({"type": "paragraph", "content": [{"type": "text", "text": text}]})
    }

    #[test]
    fn test_deep_blocks_truncate_without_losing_following_content() {
        for kind in [
            "bulletList",
            "orderedList",
            "listItem",
            "unknownContainer",
            "blockquote",
            "panel",
            "table",
            "expand",
            "nestedExpand",
            "taskList",
            "taskItem",
            "decisionList",
            "decisionItem",
            "layoutSection",
            "layoutColumn",
            "bodiedExtension",
            "multiBodiedExtension",
            "extensionFrame",
        ] {
            let mut nested = paragraph("deep-only-leaf");
            for _ in 0..64 {
                let child = match kind {
                    "bulletList" | "orderedList" => block("listItem", vec![nested]),
                    "taskList" => block("taskItem", vec![nested]),
                    "decisionList" => block("decisionItem", vec![nested]),
                    "layoutSection" => block("layoutColumn", vec![nested]),
                    "table" => block("tableRow", vec![block("tableCell", vec![nested])]),
                    _ => nested,
                };
                nested = block(kind, vec![child]);
            }
            let mut document = block("doc", vec![nested, paragraph("following-sibling")]);
            document["version"] = 1.into();

            let result = adf_to_markdown(&document);
            assert!(
                result.contains("[Content truncated: max depth exceeded]"),
                "{kind}: missing truncation marker"
            );
            assert!(
                !result.contains("deep-only-leaf"),
                "{kind}: depth was not bounded"
            );
            assert!(
                result.ends_with("following-sibling"),
                "{kind}: lost following content"
            );
        }
    }

    #[test]
    fn test_mixed_blocks_preserve_list_indentation() {
        let quote = block(
            "blockquote",
            vec![
                paragraph("Quote"),
                block(
                    "bulletList",
                    vec![block(
                        "listItem",
                        vec![
                            paragraph("Outer"),
                            block(
                                "orderedList",
                                vec![block("listItem", vec![paragraph("Inner")])],
                            ),
                        ],
                    )],
                ),
            ],
        );
        let table = block(
            "table",
            vec![
                block(
                    "tableRow",
                    vec![block("tableHeader", vec![paragraph("Column")])],
                ),
                block(
                    "tableRow",
                    vec![block(
                        "tableCell",
                        vec![block(
                            "bulletList",
                            vec![block("listItem", vec![paragraph("Cell item")])],
                        )],
                    )],
                ),
            ],
        );
        let mut document = block("doc", vec![quote, table, paragraph("After")]);
        document["version"] = 1.into();

        assert_eq!(
            adf_to_markdown(&document),
            "> Quote\n> - Outer \n>   1. Inner\n\n| Column |\n| --- |\n| - Cell item |\n\nAfter"
        );
    }
}
