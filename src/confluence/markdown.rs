use htmd::HtmlToMarkdown;

const IMAGE_PLACEHOLDER_PREFIX: &str = "IMGPLACEHOLDER";
const IMAGE_PLACEHOLDER_SUFFIX: &str = "END";

/// Convert Confluence HTML to Markdown
pub fn convert_to_markdown(html: &str) -> String {
    let converter = HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style"])
        .build();

    let cleaned = clean_confluence_html(html);
    let (cleaned, image_map) = extract_images(&cleaned);

    let markdown = converter
        .convert(&cleaned)
        .unwrap_or_else(|_| html.to_string());

    let markdown = restore_images(&markdown, &image_map);
    normalize_whitespace(&markdown)
}

fn clean_confluence_html(html: &str) -> String {
    let mut result = html.to_string();

    while let Some(start) = result.find("<ac:emoticon") {
        if let Some(end) = result[start..].find("/>") {
            result = format!("{}{}", &result[..start], &result[start + end + 2..]);
        } else if let Some(end) = result[start..].find("</ac:emoticon>") {
            result = format!("{}{}", &result[..start], &result[start + end + 14..]);
        } else {
            break;
        }
    }

    while let Some(start) = result.find("<ac:image") {
        if let Some(end) = result[start..].find("</ac:image>") {
            let block = &result[start..start + end + 11];
            let replacement = extract_confluence_image(block);
            result = format!(
                "{}{}{}",
                &result[..start],
                replacement,
                &result[start + end + 11..]
            );
        } else if let Some(end) = result[start..].find("/>") {
            result = format!("{}{}", &result[..start], &result[start + end + 2..]);
        } else {
            break;
        }
    }

    while let Some(start) = result.find("<ac:link") {
        if let Some(end) = result[start..].find("</ac:link>") {
            let block = &result[start..start + end + 10];
            let replacement = extract_link_text(block);
            result = format!(
                "{}{}{}",
                &result[..start],
                replacement,
                &result[start + end + 10..]
            );
        } else {
            break;
        }
    }

    while let Some(start) = result.find("<ac:structured-macro") {
        if let Some(end) = result[start..].find("</ac:structured-macro>") {
            let block = &result[start..start + end + 22];
            let replacement = process_macro(block);
            result = format!(
                "{}{}{}",
                &result[..start],
                replacement,
                &result[start + end + 22..]
            );
        } else {
            break;
        }
    }

    remove_confluence_tags(&result)
}

fn extract_confluence_image(block: &str) -> String {
    if let Some(start) = block.find("ri:filename=\"") {
        let rest = &block[start + 13..];
        if let Some(end) = rest.find('"') {
            return format!("<img alt=\"{}\"/>", &rest[..end]);
        }
    }
    String::new()
}

fn extract_link_text(block: &str) -> String {
    if let Some(start) = block.find("<ac:plain-text-link-body>") {
        let rest = &block[start + 25..];
        if let Some(end) = rest.find("</ac:plain-text-link-body>") {
            return rest[..end]
                .replace("<![CDATA[", "")
                .replace("]]>", "")
                .trim()
                .to_string();
        }
    }

    if let Some(start) = block.find("ri:content-title=\"") {
        let rest = &block[start + 18..];
        if let Some(end) = rest.find('"') {
            return rest[..end].to_string();
        }
    }

    String::new()
}

fn process_macro(block: &str) -> String {
    let macro_name = if let Some(start) = block.find("ac:name=\"") {
        let rest = &block[start + 9..];
        rest.split('"').next().unwrap_or("")
    } else {
        ""
    };

    match macro_name {
        "code" | "noformat" => {
            if let Some(body) = extract_plain_text_body(block) {
                return format!("<pre><code>{}</code></pre>", body);
            }
        }
        "info" | "note" | "warning" | "tip" => {
            if let Some(body) = extract_rich_text_body(block) {
                return format!("> **{}**: {}", macro_name.to_uppercase(), body);
            }
        }
        "toc" => return String::new(),
        "expand" => {
            if let Some(body) = extract_rich_text_body(block) {
                return body;
            }
        }
        _ => {}
    }

    extract_rich_text_body(block).unwrap_or_default()
}

fn extract_plain_text_body(block: &str) -> Option<String> {
    let start = block.find("<ac:plain-text-body>")?;
    let rest = &block[start + 20..];
    let end = rest.find("</ac:plain-text-body>")?;
    Some(
        rest[..end]
            .replace("<![CDATA[", "")
            .replace("]]>", "")
            .trim()
            .to_string(),
    )
}

fn extract_rich_text_body(block: &str) -> Option<String> {
    let start = block.find("<ac:rich-text-body>")?;
    let rest = &block[start + 19..];
    let end = rest.find("</ac:rich-text-body>")?;
    Some(remove_confluence_tags(&rest[..end]).trim().to_string())
}

fn extract_images(html: &str) -> (String, Vec<String>) {
    let mut result = html.to_string();
    let mut image_map = Vec::new();

    while let Some(start) = result.find("<img") {
        let rest = &result[start..];
        let end_pos = if let Some(pos) = rest.find("/>") {
            pos + 2
        } else if let Some(pos) = rest.find('>') {
            pos + 1
        } else {
            break;
        };

        let tag = &rest[..end_pos];
        let alt = extract_alt_attr(tag);

        if !alt.is_empty() {
            let placeholder = format!(
                "{}{}{}",
                IMAGE_PLACEHOLDER_PREFIX,
                image_map.len(),
                IMAGE_PLACEHOLDER_SUFFIX
            );
            image_map.push(alt);
            result = format!(
                "{}{}{}",
                &result[..start],
                placeholder,
                &result[start + end_pos..]
            );
        } else {
            result = format!("{}{}", &result[..start], &result[start + end_pos..]);
        }
    }

    (result, image_map)
}

fn extract_alt_attr(tag: &str) -> String {
    if let Some(start) = tag.find("alt=\"") {
        let rest = &tag[start + 5..];
        if let Some(end) = rest.find('"') {
            return rest[..end].to_string();
        }
    }
    String::new()
}

fn restore_images(markdown: &str, image_map: &[String]) -> String {
    let mut result = markdown.to_string();
    for (i, alt) in image_map.iter().enumerate() {
        let placeholder = format!(
            "{}{}{}",
            IMAGE_PLACEHOLDER_PREFIX, i, IMAGE_PLACEHOLDER_SUFFIX
        );
        result = result.replace(&placeholder, &format!("[Image: {}]", alt));
    }
    result
}

fn remove_confluence_tags(html: &str) -> String {
    let mut result = html.to_string();
    let patterns = ["<ac:", "<ri:", "</ac:", "</ri:"];

    for pattern in patterns {
        while let Some(start) = result.find(pattern) {
            if let Some(end) = result[start..].find('>') {
                result = format!("{}{}", &result[..start], &result[start + end + 1..]);
            } else {
                break;
            }
        }
    }
    result
}

fn normalize_whitespace(text: &str) -> String {
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

    #[test]
    fn test_basic_html_to_markdown() {
        let html = "<p>Hello <strong>world</strong></p>";
        let result = convert_to_markdown(html);
        assert!(result.contains("**world**"));
    }

    #[test]
    fn test_heading_conversion() {
        let html = "<h1>Title</h1><h2>Subtitle</h2>";
        let result = convert_to_markdown(html);
        assert!(result.contains("# Title"));
        assert!(result.contains("## Subtitle"));
    }

    #[test]
    fn test_table_conversion() {
        let html = r#"<table><thead><tr><th>A</th><th>B</th></tr></thead><tbody><tr><td>1</td><td>2</td></tr></tbody></table>"#;
        let result = convert_to_markdown(html);
        assert!(result.contains("|"));
        assert!(result.contains("A"));
        assert!(result.contains("B"));
    }

    #[test]
    fn test_image_alt_text() {
        let html = r#"<img src="x.png" alt="screenshot"/>"#;
        let result = convert_to_markdown(html);
        assert_eq!(result, "[Image: screenshot]");
    }

    #[test]
    fn test_image_empty_alt() {
        let html = r#"<img src="x.png" alt=""/>"#;
        let result = convert_to_markdown(html);
        assert!(result.is_empty() || !result.contains("[Image:"));
    }

    #[test]
    fn test_script_removal() {
        let html = "<p>Text</p><script>alert('x')</script>";
        let result = convert_to_markdown(html);
        assert!(!result.contains("script"));
        assert!(!result.contains("alert"));
    }

    #[test]
    fn test_confluence_code_macro() {
        let html = r#"<ac:structured-macro ac:name="code"><ac:plain-text-body><![CDATA[let x = 1;]]></ac:plain-text-body></ac:structured-macro>"#;
        let result = convert_to_markdown(html);
        assert!(result.contains("```"));
        assert!(result.contains("let x = 1;"));
    }

    #[test]
    fn test_confluence_info_panel() {
        let html = r#"<ac:structured-macro ac:name="info"><ac:rich-text-body><p>Important note</p></ac:rich-text-body></ac:structured-macro>"#;
        let result = convert_to_markdown(html);
        assert!(result.contains("INFO"));
        assert!(result.contains("Important note"));
    }

    #[test]
    fn test_confluence_image() {
        let html = r#"<ac:image><ri:attachment ri:filename="diagram.png"/></ac:image>"#;
        let result = convert_to_markdown(html);
        assert!(result.contains("[Image: diagram.png]"));
    }

    #[test]
    fn test_normalize_whitespace() {
        let text = "Line 1\n\n\n\nLine 2\n\n";
        let result = normalize_whitespace(text);
        assert_eq!(result, "Line 1\n\nLine 2");
    }

    #[test]
    fn test_list_conversion() {
        let html = "<ul><li>Item 1</li><li>Item 2</li></ul>";
        let result = convert_to_markdown(html);
        assert!(result.contains("- ") || result.contains("* "));
    }

    #[test]
    fn test_ordered_list() {
        let html = "<ol><li>First</li><li>Second</li></ol>";
        let result = convert_to_markdown(html);
        assert!(result.contains("1.") || result.contains("1)"));
    }

    #[test]
    fn test_link_conversion() {
        let html = r#"<a href="https://example.com">Example</a>"#;
        let result = convert_to_markdown(html);
        assert!(result.contains("[Example]"));
        assert!(result.contains("https://example.com"));
    }
}
