use super::cleanup::clean_binary_data;
use std::collections::HashMap;

pub fn process_macro(block: &str) -> String {
    let macro_name = extract_macro_name(block).unwrap_or_default();
    let params = extract_parameters(block);
    let body = extract_body(block);

    match macro_name {
        "code" | "noformat" => format_code_block(&params, &body),
        "info" | "note" | "warning" | "tip" | "error" => {
            format_panel(&macro_name.to_uppercase(), &params, &body)
        }
        "toc" => String::new(),
        "expand" => format_expand(&params, &body),
        "anchor" => format_anchor(&params),
        "jira" => format_jira(&params),
        "status" => format_status(&params),
        "drawio" => format_diagram("Draw.io", &params, "diagramName", "diagramDisplayName"),
        "gliffy" => format_diagram("Gliffy", &params, "name", "displayName"),
        "lucidchart" => format_lucidchart(&params),
        "miro" => format_miro(&params),
        "plantuml" => format_plantuml(&body),
        "children" => format_children(&params),
        "pagetree" => "[Page Tree]".into(),
        "recently-updated" => "[Recently Updated]".into(),
        "widget" | "iframe" | "html" => format_embed(&params),
        _ => format_unknown_macro(macro_name, &params, &body),
    }
}

fn extract_macro_name(block: &str) -> Option<&str> {
    let start = block.find("ac:name=\"")? + 9;
    let rest = &block[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn extract_parameters(block: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    let mut search_start = 0;

    while let Some(param_start) = block[search_start..].find("<ac:parameter ac:name=\"") {
        let abs_start = search_start + param_start;
        let name_start = abs_start + 23;

        if let Some(name_end_rel) = block[name_start..].find('"') {
            let name = &block[name_start..name_start + name_end_rel];

            if !name.is_empty() {
                let after_name = name_start + name_end_rel + 1;

                // Check for self-closing tag
                if let Some(close_pos) = block[after_name..].find('>') {
                    let tag_end = after_name + close_pos;

                    if block[after_name..tag_end].contains("/>")
                        || block[tag_end..].starts_with("/>")
                    {
                        search_start = tag_end + 2;
                        continue;
                    }

                    // Find closing tag
                    let content_start = tag_end + 1;
                    if let Some(close_tag) = block[content_start..].find("</ac:parameter>") {
                        let value = &block[content_start..content_start + close_tag];
                        params.insert(name.to_string(), value.trim().to_string());
                        search_start = content_start + close_tag + 15;
                        continue;
                    }
                }
            }
        }
        search_start = abs_start + 23;
    }

    params
}

fn extract_body(block: &str) -> String {
    // Try rich-text-body first
    if let Some(body) = extract_body_tag(block, "ac:rich-text-body") {
        return clean_binary_data(&body);
    }

    // Then plain-text-body
    if let Some(body) = extract_body_tag(block, "ac:plain-text-body") {
        return clean_binary_data(&body);
    }

    String::new()
}

fn extract_body_tag(block: &str, tag: &str) -> Option<String> {
    let open_tag = format!("<{}>", tag);
    let close_tag = format!("</{}>", tag);

    let start = block.find(&open_tag)? + open_tag.len();
    let rest = &block[start..];
    let end = rest.find(&close_tag)?;

    Some(rest[..end].to_string())
}

fn format_code_block(params: &HashMap<String, String>, body: &str) -> String {
    let language = params.get("language").map(|s| s.as_str()).unwrap_or("");
    let title = params.get("title");

    let mut result = String::new();
    if let Some(t) = title {
        result.push_str(&format!("**{}**\n", t));
    }
    result.push_str(&format!("```{}\n{}\n```", language, body.trim()));
    result
}

fn format_panel(panel_type: &str, params: &HashMap<String, String>, body: &str) -> String {
    let title = params.get("title");
    let content = body.trim();

    match title {
        Some(t) => format!("> **{} - {}**: {}", panel_type, t, content),
        None => format!("> **{}**: {}", panel_type, content),
    }
}

fn format_expand(params: &HashMap<String, String>, body: &str) -> String {
    let title = params.get("title").map(|s| s.as_str()).unwrap_or("Details");
    let content = body.trim();

    if content.is_empty() {
        format!("**{}**", title)
    } else {
        format!("**{}**\n\n{}", title, content)
    }
}

fn format_anchor(params: &HashMap<String, String>) -> String {
    params
        .get("")
        .or_else(|| params.get("name"))
        .map(|name| format!("<a id=\"{}\"></a>", name))
        .unwrap_or_default()
}

fn format_jira(params: &HashMap<String, String>) -> String {
    let key = params.get("key").map(|s| s.as_str()).unwrap_or("JIRA");
    let server = params.get("server").or_else(|| params.get("serverId"));

    match server {
        Some(s) => format!("[{}]({})", key, s),
        None => format!("[JIRA: {}]", key),
    }
}

fn format_status(params: &HashMap<String, String>) -> String {
    let title = params.get("title").map(|s| s.as_str()).unwrap_or("STATUS");
    let color = params
        .get("colour")
        .or_else(|| params.get("color"))
        .map(|s| s.as_str())
        .unwrap_or("Grey");

    let indicator = match color.to_lowercase().as_str() {
        "green" => "[OK]",
        "yellow" => "[WARN]",
        "red" => "[ERR]",
        "blue" => "[INFO]",
        _ => "[STATUS]",
    };

    format!("{} {}", indicator, title.to_uppercase())
}

fn format_diagram(tool: &str, params: &HashMap<String, String>, key1: &str, key2: &str) -> String {
    let name = params
        .get(key1)
        .or_else(|| params.get(key2))
        .map(|s| s.as_str())
        .unwrap_or("diagram");

    format!("[{}: {}]", tool, name)
}

fn format_lucidchart(params: &HashMap<String, String>) -> String {
    match params.get("documentId") {
        Some(id) => format!("[Lucidchart](https://lucid.app/documents/view/{})", id),
        None => "[Lucidchart]".into(),
    }
}

fn format_miro(params: &HashMap<String, String>) -> String {
    match params.get("boardId") {
        Some(id) => format!("[Miro](https://miro.com/app/board/{})", id),
        None => "[Miro Board]".into(),
    }
}

fn format_plantuml(body: &str) -> String {
    let content = body.trim();
    if content.is_empty() {
        "[PlantUML Diagram]".into()
    } else {
        format!("```plantuml\n{}\n```", content)
    }
}

fn format_children(params: &HashMap<String, String>) -> String {
    let depth = params.get("depth").map(|s| s.as_str()).unwrap_or("1");
    format!("[Child Pages (depth: {})]", depth)
}

fn format_embed(params: &HashMap<String, String>) -> String {
    let url = params
        .get("url")
        .or_else(|| params.get("src"))
        .or_else(|| params.get("name"));

    match url {
        Some(u) => format!("[Embed: {}]", u),
        None => "[Embedded Content]".into(),
    }
}

fn format_unknown_macro(name: &str, params: &HashMap<String, String>, body: &str) -> String {
    let body = body.trim();

    // If body has meaningful content, return it
    if !body.is_empty() && body.len() > 3 {
        return body.to_string();
    }

    // Otherwise, show macro info
    let meaningful: Vec<String> = params
        .iter()
        .filter(|(k, _)| matches!(k.as_str(), "title" | "name" | "key" | "url"))
        .map(|(k, v)| format!("{}={}", k, v))
        .collect();

    if meaningful.is_empty() {
        format!("[Macro: {}]", name)
    } else {
        format!("[Macro: {} ({})]", name, meaningful.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_macro_name() {
        let block = r#"<ac:structured-macro ac:name="code">"#;
        assert_eq!(extract_macro_name(block), Some("code"));
    }

    #[test]
    fn test_extract_parameters() {
        let block = r#"<ac:parameter ac:name="language">rust</ac:parameter>"#;
        let params = extract_parameters(block);
        assert_eq!(params.get("language"), Some(&"rust".to_string()));
    }

    #[test]
    fn test_code_macro() {
        let block = r#"<ac:structured-macro ac:name="code">
            <ac:parameter ac:name="language">rust</ac:parameter>
            <ac:plain-text-body>let x = 1;</ac:plain-text-body>
        </ac:structured-macro>"#;
        let result = process_macro(block);
        assert!(result.contains("```rust"));
        assert!(result.contains("let x = 1;"));
    }

    #[test]
    fn test_info_panel() {
        let block = r#"<ac:structured-macro ac:name="info">
            <ac:rich-text-body>Important info</ac:rich-text-body>
        </ac:structured-macro>"#;
        let result = process_macro(block);
        assert!(result.contains("> **INFO**"));
        assert!(result.contains("Important info"));
    }

    #[test]
    fn test_drawio() {
        let block = r#"<ac:structured-macro ac:name="drawio">
            <ac:parameter ac:name="diagramName">architecture</ac:parameter>
        </ac:structured-macro>"#;
        let result = process_macro(block);
        assert_eq!(result, "[Draw.io: architecture]");
    }

    #[test]
    fn test_status() {
        let block = r#"<ac:structured-macro ac:name="status">
            <ac:parameter ac:name="title">Done</ac:parameter>
            <ac:parameter ac:name="colour">Green</ac:parameter>
        </ac:structured-macro>"#;
        let result = process_macro(block);
        assert_eq!(result, "[OK] DONE");
    }

    #[test]
    fn test_jira() {
        let block = r#"<ac:structured-macro ac:name="jira">
            <ac:parameter ac:name="key">PROJ-123</ac:parameter>
        </ac:structured-macro>"#;
        let result = process_macro(block);
        assert_eq!(result, "[JIRA: PROJ-123]");
    }

    #[test]
    fn test_toc() {
        let block = r#"<ac:structured-macro ac:name="toc" />"#;
        let result = process_macro(block);
        assert!(result.is_empty());
    }

    #[test]
    fn test_expand() {
        let block = r#"<ac:structured-macro ac:name="expand">
            <ac:parameter ac:name="title">Click to expand</ac:parameter>
            <ac:rich-text-body>Hidden content</ac:rich-text-body>
        </ac:structured-macro>"#;
        let result = process_macro(block);
        assert!(result.contains("**Click to expand**"));
        assert!(result.contains("Hidden content"));
    }
}
