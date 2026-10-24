use super::MAX_DEPTH;
use super::inline::convert_inline_nodes;
use serde_json::Value;

pub fn convert_block_node(node: &Value, depth: usize, list_depth: usize) -> Option<String> {
    if depth > MAX_DEPTH {
        return Some("[Content truncated: max depth exceeded]".into());
    }

    let node_type = node.get("type")?.as_str()?;

    match node_type {
        "paragraph" => convert_paragraph(node),
        "heading" => convert_heading(node),
        "bulletList" => convert_bullet_list(node, depth, list_depth),
        "orderedList" => convert_ordered_list(node, depth, list_depth),
        "listItem" => convert_list_item(node, depth, list_depth),
        "codeBlock" => convert_code_block(node),
        "blockquote" => convert_blockquote(node, depth),
        "rule" => Some("---".into()),
        "panel" => convert_panel(node, depth),
        "table" => convert_table(node, depth),
        "mediaSingle" | "mediaGroup" => convert_media(node),
        "expand" | "nestedExpand" => convert_expand(node, depth),
        "taskList" => convert_task_list(node, depth),
        "taskItem" => convert_task_item(node, depth),
        "decisionList" => convert_decision_list(node, depth),
        "decisionItem" => convert_decision_item(node, depth),
        "layoutSection" => convert_layout_section(node, depth),
        "layoutColumn" => convert_layout_column(node, depth),
        "embedCard" => convert_embed_card(node),
        "bodiedExtension" | "multiBodiedExtension" => convert_extension(node, depth),
        "extensionFrame" => convert_extension_frame(node, depth),
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

fn convert_children(node: &Value, depth: usize) -> String {
    node.get("content")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|n| convert_block_node(n, depth + 1, 0))
                .collect::<Vec<_>>()
                .join("\n\n")
        })
        .unwrap_or_default()
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

fn convert_bullet_list(node: &Value, depth: usize, list_depth: usize) -> Option<String> {
    let items = node.get("content")?.as_array()?;
    let lines: Vec<String> = items
        .iter()
        .filter_map(|item| {
            let content = convert_block_node(item, depth + 1, list_depth)?;
            Some(format!("{}- {}", "  ".repeat(list_depth), content))
        })
        .collect();

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn convert_ordered_list(node: &Value, depth: usize, list_depth: usize) -> Option<String> {
    let items = node.get("content")?.as_array()?;
    let lines: Vec<String> = items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| {
            let content = convert_block_node(item, depth + 1, list_depth)?;
            Some(format!("{}{}. {}", "  ".repeat(list_depth), i + 1, content))
        })
        .collect();

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn convert_list_item(node: &Value, depth: usize, list_depth: usize) -> Option<String> {
    let content = node.get("content")?.as_array()?;
    let mut parts: Vec<String> = Vec::new();

    for child in content {
        let child_type = child.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if matches!(child_type, "bulletList" | "orderedList") {
            if let Some(list) = convert_block_node(child, depth + 1, list_depth + 1) {
                parts.push(format!("\n{}", list));
            }
        } else if let Some(text) = convert_block_node(child, depth + 1, 0) {
            parts.push(text);
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

    let code = node
        .get("content")
        .and_then(|c| c.as_array())
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

fn convert_blockquote(node: &Value, depth: usize) -> Option<String> {
    let content = node.get("content")?.as_array()?;
    let lines: Vec<String> = content
        .iter()
        .filter_map(|child| convert_block_node(child, depth + 1, 0))
        .flat_map(|text| text.lines().map(|l| format!("> {}", l)).collect::<Vec<_>>())
        .collect();

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn convert_panel(node: &Value, depth: usize) -> Option<String> {
    let panel_type = node
        .get("attrs")
        .and_then(|a| a.get("panelType"))
        .and_then(|p| p.as_str())
        .unwrap_or("info")
        .to_uppercase();

    let content = node.get("content")?.as_array()?;
    let text: Vec<String> = content
        .iter()
        .filter_map(|child| convert_block_node(child, depth + 1, 0))
        .collect();

    if text.is_empty() {
        None
    } else {
        Some(format!("> **{}**: {}", panel_type, text.join(" ")))
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

fn convert_cell_content(cell: &Value, depth: usize) -> String {
    let content = cell
        .get("content")
        .and_then(|c| c.as_array())
        .map(|content| {
            content
                .iter()
                .filter_map(|n| convert_block_node(n, depth + 1, 0))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();

    // Escape pipe characters to prevent breaking table structure
    content.replace('|', "\\|")
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

    let content = node.get("content")?.as_array()?;
    let text: Vec<String> = content
        .iter()
        .filter_map(|child| convert_block_node(child, depth + 1, 0))
        .collect();

    if text.is_empty() {
        None
    } else {
        Some(format!("**{}**\n\n{}", title, text.join("\n\n")))
    }
}

fn convert_task_list(node: &Value, depth: usize) -> Option<String> {
    let items = node.get("content")?.as_array()?;
    let lines: Vec<String> = items
        .iter()
        .filter_map(|item| convert_block_node(item, depth + 1, 0))
        .collect();

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn convert_task_item(node: &Value, depth: usize) -> Option<String> {
    let attrs = node.get("attrs");
    let state = attrs
        .and_then(|a| a.get("state"))
        .and_then(|s| s.as_str())
        .unwrap_or("TODO");

    let checkbox = if state == "DONE" { "[x]" } else { "[ ]" };

    let content = node
        .get("content")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|n| convert_block_node(n, depth + 1, 0))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();

    Some(format!("- {} {}", checkbox, content))
}

fn convert_decision_list(node: &Value, depth: usize) -> Option<String> {
    let items = node.get("content")?.as_array()?;
    let lines: Vec<String> = items
        .iter()
        .filter_map(|item| convert_block_node(item, depth + 1, 0))
        .collect();

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn convert_decision_item(node: &Value, depth: usize) -> Option<String> {
    let attrs = node.get("attrs");
    let state = attrs
        .and_then(|a| a.get("state"))
        .and_then(|s| s.as_str())
        .unwrap_or("DECIDED");

    let icon = if state == "DECIDED" { "[x]" } else { "[ ]" };

    let content = node
        .get("content")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|n| convert_block_node(n, depth + 1, 0))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();

    Some(format!("- {} {}", icon, content))
}

fn convert_layout_section(node: &Value, depth: usize) -> Option<String> {
    let content = node.get("content")?.as_array()?;
    let columns: Vec<String> = content
        .iter()
        .filter_map(|column| convert_block_node(column, depth + 1, 0))
        .collect();

    if columns.is_empty() {
        None
    } else {
        Some(columns.join("\n\n---\n\n"))
    }
}

fn convert_layout_column(node: &Value, depth: usize) -> Option<String> {
    let content = node.get("content")?.as_array()?;
    let text: Vec<String> = content
        .iter()
        .filter_map(|n| convert_block_node(n, depth + 1, 0))
        .collect();

    if text.is_empty() {
        None
    } else {
        Some(text.join("\n\n"))
    }
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

fn convert_extension_frame(node: &Value, depth: usize) -> Option<String> {
    convert_children(node, depth).into()
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
        assert_eq!(convert_block_node(&node, 0, 0), Some("Hello world".into()));
    }

    #[test]
    fn test_heading() {
        let node = json!({
            "type": "heading",
            "attrs": {"level": 2},
            "content": [{"type": "text", "text": "Title"}]
        });
        assert_eq!(convert_block_node(&node, 0, 0), Some("## Title".into()));
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
        let result = convert_block_node(&node, 0, 0).unwrap();
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
        let result = convert_block_node(&node, 0, 0).unwrap();
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
        let result = convert_block_node(&node, 0, 0).unwrap();
        assert!(result.contains("```rust"));
        assert!(result.contains("fn main() {}"));
    }

    #[test]
    fn test_blockquote() {
        let node = json!({
            "type": "blockquote",
            "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Quote"}]}]
        });
        let result = convert_block_node(&node, 0, 0).unwrap();
        assert!(result.contains("> Quote"));
    }

    #[test]
    fn test_rule() {
        let node = json!({"type": "rule"});
        assert_eq!(convert_block_node(&node, 0, 0), Some("---".into()));
    }

    #[test]
    fn test_panel() {
        let node = json!({
            "type": "panel",
            "attrs": {"panelType": "info"},
            "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Note"}]}]
        });
        let result = convert_block_node(&node, 0, 0).unwrap();
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
        let result = convert_block_node(&node, 0, 0).unwrap();
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
        let result = convert_block_node(&node, 0, 0).unwrap();
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
        let result = convert_block_node(&node, 0, 0).unwrap();
        assert!(result.contains("**Click to expand**"));
        assert!(result.contains("Hidden content"));
    }

    #[test]
    fn test_embed_card() {
        let node = json!({
            "type": "embedCard",
            "attrs": {"url": "https://example.com"}
        });
        let result = convert_block_node(&node, 0, 0).unwrap();
        assert_eq!(result, "[https://example.com](https://example.com)");
    }
}
