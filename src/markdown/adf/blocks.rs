use super::MAX_DEPTH;
use super::inline::{convert_inline_node, convert_inline_nodes};
use crate::markdown::common::fenced_code_block;
use serde_json::Value;

pub fn convert_block_node(node: &Value, depth: usize) -> Option<String> {
    if depth > MAX_DEPTH {
        return Some("[Content truncated: max depth exceeded]".into());
    }

    let node_type = node.get("type")?.as_str()?;

    match node_type {
        "paragraph" => convert_paragraph(node),
        "heading" => convert_heading(node),
        "bulletList" => convert_bullet_list(node, depth),
        "orderedList" => convert_ordered_list(node, depth),
        "listItem" => convert_list_item(node, depth),
        "codeBlock" => convert_code_block(node),
        "blockquote" => non_empty(quote(&convert_children(node, depth))),
        "rule" => Some("---".into()),
        "panel" => convert_panel(node, depth),
        "table" => convert_table(node, depth),
        "mediaSingle" | "mediaGroup" => convert_media(node),
        "expand" | "nestedExpand" => convert_expand(node, depth),
        "taskList" => convert_task_list(node, depth),
        "taskItem" => convert_checked_item(node, depth, "state", "DONE"),
        "decisionList" => non_empty(child_blocks(node, depth).join("\n")),
        "decisionItem" => convert_checked_item(node, depth, "state", "DECIDED"),
        "layoutSection" => non_empty(child_blocks(node, depth).join("\n\n---\n\n")),
        "layoutColumn" => non_empty(convert_children(node, depth)),
        "embedCard" => convert_embed_card(node),
        "bodiedExtension" | "multiBodiedExtension" => convert_extension(node, depth),
        "extensionFrame" => Some(convert_children(node, depth)),
        unknown => {
            let content = convert_children(node, depth);
            if content.is_empty() {
                None
            } else {
                Some(format!("<!-- Unsupported: {} -->\n{}", unknown, content))
            }
        }
    }
}

/// The converted child blocks of `node`, empty ones left out.
fn child_blocks(node: &Value, depth: usize) -> Vec<String> {
    node.get("content")
        .and_then(|c| c.as_array())
        .map(|children| {
            children
                .iter()
                .filter_map(|child| convert_block_node(child, depth + 1))
                .collect()
        })
        .unwrap_or_default()
}

/// Child blocks separated as Markdown separates blocks: by a blank line.
fn convert_children(node: &Value, depth: usize) -> String {
    child_blocks(node, depth).join("\n\n")
}

fn non_empty(content: String) -> Option<String> {
    (!content.is_empty()).then_some(content)
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
    let items: Vec<String> = child_blocks(node, depth)
        .iter()
        .map(|item| list_item("- ", item))
        .collect();
    non_empty(items.join("\n"))
}

fn convert_ordered_list(node: &Value, depth: usize) -> Option<String> {
    let start = node
        .get("attrs")
        .and_then(|a| a.get("order"))
        .and_then(|o| o.as_u64())
        .unwrap_or(1);
    let items: Vec<String> = child_blocks(node, depth)
        .iter()
        .zip(start..)
        .map(|(item, number)| list_item(&format!("{number}. "), item))
        .collect();
    non_empty(items.join("\n"))
}

/// A list item's blocks: a nested list follows its paragraph directly, which
/// keeps a list tight; other blocks are separated by a blank line.
fn convert_list_item(node: &Value, depth: usize) -> Option<String> {
    let children = node.get("content")?.as_array()?;
    let mut content = String::new();
    for child in children {
        let Some(block) = convert_block_node(child, depth + 1) else {
            continue;
        };
        if !content.is_empty() {
            let is_list = matches!(
                child.get("type").and_then(|t| t.as_str()),
                Some("bulletList" | "orderedList")
            );
            content.push_str(if is_list { "\n" } else { "\n\n" });
        }
        content.push_str(&block);
    }
    non_empty(content)
}

/// `content` under a list marker, its later lines indented to the marker's
/// width so they stay inside the item.
fn list_item(marker: &str, content: &str) -> String {
    let indent = " ".repeat(marker.len());
    let mut lines = content.lines();
    let mut item = format!("{marker}{}", lines.next().unwrap_or_default());
    for line in lines {
        item.push('\n');
        if !line.is_empty() {
            item.push_str(&indent);
            item.push_str(line);
        }
    }
    item
}

fn quote(content: &str) -> String {
    content
        .lines()
        .map(|line| {
            if line.is_empty() {
                ">".to_string()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn convert_code_block(node: &Value) -> Option<String> {
    let language = node
        .get("attrs")
        .and_then(|a| a.get("language"))
        .and_then(|l| l.as_str())
        .unwrap_or("");

    let code: String = node
        .get("content")
        .and_then(|c| c.as_array())
        .map(|nodes| {
            nodes
                .iter()
                .filter_map(|n| n.get("text").and_then(|t| t.as_str()))
                .collect()
        })
        .unwrap_or_default();

    Some(fenced_code_block(language, &code))
}

fn convert_panel(node: &Value, depth: usize) -> Option<String> {
    let panel_type = node
        .get("attrs")
        .and_then(|a| a.get("panelType"))
        .and_then(|p| p.as_str())
        .unwrap_or("info")
        .to_uppercase();

    let content = convert_children(node, depth);
    if content.is_empty() {
        None
    } else {
        Some(quote(&format!("**{panel_type}**\n\n{content}")))
    }
}

fn convert_table(node: &Value, depth: usize) -> Option<String> {
    let rows = node.get("content")?.as_array()?;
    if rows.is_empty() {
        return None;
    }

    let mut result: Vec<String> = Vec::new();
    let mut col_count = 0;
    let mut is_first_row = true;

    for row in rows {
        let cells = row.get("content").and_then(|c| c.as_array())?;
        let mut row_cells: Vec<String> = Vec::new();

        for cell in cells {
            let attrs = cell.get("attrs");
            let colspan = attrs
                .and_then(|a| a.get("colspan"))
                .and_then(|c| c.as_u64())
                .unwrap_or(1) as usize;

            let content = convert_cell_content(cell, depth + 2);

            for i in 0..colspan {
                if i == 0 {
                    row_cells.push(content.clone());
                } else {
                    row_cells.push(String::new());
                }
            }
        }

        if is_first_row {
            col_count = row_cells.len();
        }

        result.push(format!("| {} |", row_cells.join(" | ")));

        if is_first_row {
            let separator: Vec<&str> = vec!["---"; col_count];
            result.push(format!("| {} |", separator.join(" | ")));
            is_first_row = false;
        }
    }

    Some(result.join("\n"))
}

/// A table row is one line, so a cell's blocks and line breaks become spaces
/// and its pipes are escaped.
fn convert_cell_content(cell: &Value, depth: usize) -> String {
    child_blocks(cell, depth)
        .join(" ")
        .replace("\\\n", " ")
        .replace('\n', " ")
        .replace('|', "\\|")
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
    Some("[Media]".into())
}

fn convert_expand(node: &Value, depth: usize) -> Option<String> {
    let title = node
        .get("attrs")
        .and_then(|a| a.get("title"))
        .and_then(|t| t.as_str())
        .unwrap_or("Details");

    let content = convert_children(node, depth);
    if content.is_empty() {
        None
    } else {
        Some(format!("**{title}**\n\n{content}"))
    }
}

/// Task items, with a nested task list placed under the item before it.
fn convert_task_list(node: &Value, depth: usize) -> Option<String> {
    let children = node.get("content")?.as_array()?;
    let mut items: Vec<String> = Vec::new();
    for child in children {
        let Some(block) = convert_block_node(child, depth + 1) else {
            continue;
        };
        let nested = child.get("type").and_then(|t| t.as_str()) == Some("taskList");
        match items.last_mut() {
            Some(previous) if nested => {
                previous.push('\n');
                previous.push_str(&list_item("  ", &block));
            }
            _ => items.push(block),
        }
    }
    non_empty(items.join("\n"))
}

/// A task or decision item: checked when its `key` attribute is `checked`.
fn convert_checked_item(node: &Value, depth: usize, key: &str, checked: &str) -> Option<String> {
    let state = node
        .get("attrs")
        .and_then(|a| a.get(key))
        .and_then(|s| s.as_str());
    let marker = if state == Some(checked) {
        "- [x] "
    } else {
        "- [ ] "
    };
    Some(list_item(marker, &convert_mixed_content(node, depth)))
}

/// Content that ADF allows to be inline nodes, blocks or both: each run of
/// inline nodes reads as one paragraph among the blocks.
fn convert_mixed_content(node: &Value, depth: usize) -> String {
    let mut blocks: Vec<String> = Vec::new();
    let mut paragraph = String::new();
    for child in node
        .get("content")
        .and_then(|c| c.as_array())
        .into_iter()
        .flatten()
    {
        if let Some(text) = convert_inline_node(child) {
            paragraph.push_str(&text);
            continue;
        }
        if !paragraph.trim().is_empty() {
            blocks.push(std::mem::take(&mut paragraph));
        }
        paragraph.clear();
        blocks.extend(convert_block_node(child, depth + 1));
    }
    if !paragraph.trim().is_empty() {
        blocks.push(paragraph);
    }
    blocks.join("\n\n")
}

fn convert_embed_card(node: &Value) -> Option<String> {
    let attrs = node.get("attrs")?;
    let url = attrs.get("url").and_then(|u| u.as_str()).unwrap_or("");

    if url.is_empty() {
        Some("[Embedded content]".into())
    } else {
        Some(format!("[{}]({})", url, url))
    }
}

fn convert_extension(node: &Value, depth: usize) -> Option<String> {
    let attrs = node.get("attrs");
    let extension_type = attrs
        .and_then(|a| a.get("extensionType"))
        .and_then(|t| t.as_str())
        .unwrap_or("extension");

    let content = convert_children(node, depth);
    if content.is_empty() {
        Some(format!("[Extension: {}]", extension_type))
    } else {
        Some(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_paragraph() {
        let node = json!({
            "type": "paragraph",
            "content": [{"type": "text", "text": "Hello world"}]
        });
        assert_eq!(convert_block_node(&node, 0), Some("Hello world".into()));
    }

    #[test]
    fn test_heading() {
        let node = json!({
            "type": "heading",
            "attrs": {"level": 2},
            "content": [{"type": "text", "text": "Title"}]
        });
        assert_eq!(convert_block_node(&node, 0), Some("## Title".into()));
    }

    #[test]
    fn test_bullet_list() {
        let node = json!({
            "type": "bulletList",
            "content": [
                {"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Item 1"}]}]},
                {"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Item 2"}]}]}
            ]
        });
        let result = convert_block_node(&node, 0).unwrap();
        assert!(result.contains("- Item 1"));
        assert!(result.contains("- Item 2"));
    }

    #[test]
    fn test_ordered_list() {
        let node = json!({
            "type": "orderedList",
            "content": [
                {"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "First"}]}]},
                {"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Second"}]}]}
            ]
        });
        let result = convert_block_node(&node, 0).unwrap();
        assert!(result.contains("1. First"));
        assert!(result.contains("2. Second"));
    }

    #[test]
    fn test_code_block() {
        let node = json!({
            "type": "codeBlock",
            "attrs": {"language": "rust"},
            "content": [{"type": "text", "text": "fn main() {}"}]
        });
        let result = convert_block_node(&node, 0).unwrap();
        assert!(result.contains("```rust"));
        assert!(result.contains("fn main() {}"));
    }

    #[test]
    fn test_blockquote() {
        let node = json!({
            "type": "blockquote",
            "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Quote"}]}]
        });
        let result = convert_block_node(&node, 0).unwrap();
        assert!(result.contains("> Quote"));
    }

    #[test]
    fn test_rule() {
        let node = json!({"type": "rule"});
        assert_eq!(convert_block_node(&node, 0), Some("---".into()));
    }

    #[test]
    fn test_panel() {
        let node = json!({
            "type": "panel",
            "attrs": {"panelType": "info"},
            "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Note"}]}]
        });
        let result = convert_block_node(&node, 0).unwrap();
        assert!(result.contains("> **INFO**"));
        assert!(result.contains("Note"));
    }

    #[test]
    fn test_table() {
        let node = json!({
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
        });
        let result = convert_block_node(&node, 0).unwrap();
        assert!(result.contains("| A | B |"));
        assert!(result.contains("| --- | --- |"));
        assert!(result.contains("| 1 | 2 |"));
    }

    #[test]
    fn test_task_list() {
        let node = json!({
            "type": "taskList",
            "content": [
                {"type": "taskItem", "attrs": {"state": "TODO"}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Todo"}]}]},
                {"type": "taskItem", "attrs": {"state": "DONE"}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Done"}]}]}
            ]
        });
        let result = convert_block_node(&node, 0).unwrap();
        assert!(result.contains("- [ ] Todo"));
        assert!(result.contains("- [x] Done"));
    }

    #[test]
    fn test_expand() {
        let node = json!({
            "type": "expand",
            "attrs": {"title": "Click to expand"},
            "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Hidden content"}]}]
        });
        let result = convert_block_node(&node, 0).unwrap();
        assert!(result.contains("**Click to expand**"));
        assert!(result.contains("Hidden content"));
    }

    #[test]
    fn test_embed_card() {
        let node = json!({
            "type": "embedCard",
            "attrs": {"url": "https://example.com"}
        });
        let result = convert_block_node(&node, 0).unwrap();
        assert_eq!(result, "[https://example.com](https://example.com)");
    }
}
