use super::macros::process_macro;

pub fn clean_confluence_html(html: &str) -> String {
    let mut result = html.to_string();

    result = process_emoticons(&result);
    result = process_images(&result);
    result = process_links(&result);
    result = process_macros(&result);
    result = process_tasks(&result);
    result = process_adf_extensions(&result);
    result = remove_remaining_tags(&result);

    result
}

fn process_emoticons(html: &str) -> String {
    let mut result = html.to_string();
    let mut iterations = 0;
    const MAX_ITERATIONS: usize = 1000;

    while iterations < MAX_ITERATIONS {
        let Some(start) = result.find("<ac:emoticon") else {
            break;
        };

        // Self-closing
        if let Some(rel_end) = result[start..].find("/>") {
            let end = start + rel_end + 2;
            let tag = &result[start..end];
            let replacement = extract_emoticon_shortname(tag);
            result = format!("{}{}{}", &result[..start], replacement, &result[end..]);
        } else if let Some(rel_end) = result[start..].find("</ac:emoticon>") {
            let end = start + rel_end + 14;
            let tag = &result[start..end];
            let replacement = extract_emoticon_shortname(tag);
            result = format!("{}{}{}", &result[..start], replacement, &result[end..]);
        } else {
            break;
        }
        iterations += 1;
    }
    result
}

fn extract_emoticon_shortname(tag: &str) -> String {
    if let Some(start) = tag.find("ac:name=\"") {
        let rest = &tag[start + 9..];
        if let Some(end) = rest.find('"') {
            let name = &rest[..end];
            return match name {
                "smile" | "smiley" => ":)",
                "sad" => ":(",
                "wink" => ";)",
                "laugh" => ":D",
                "thumbs-up" => "(y)",
                "thumbs-down" => "(n)",
                "tick" | "check" => "[x]",
                "cross" | "error" => "[!]",
                "warning" => "[!]",
                "information" | "info" => "(i)",
                "question" => "(?)",
                "light-on" | "idea" => "(!)",
                "star" => "(*)",
                "heart" => "<3",
                _ => name,
            }
            .to_string();
        }
    }
    String::new()
}

fn process_images(html: &str) -> String {
    let mut result = html.to_string();
    let mut iterations = 0;
    const MAX_ITERATIONS: usize = 1000;

    while iterations < MAX_ITERATIONS {
        let Some(start) = result.find("<ac:image") else {
            break;
        };

        if let Some(rel_end) = result[start..].find("</ac:image>") {
            let end = start + rel_end + 11;
            let block = &result[start..end];
            let replacement = extract_image_info(block);
            result = format!("{}{}{}", &result[..start], replacement, &result[end..]);
        } else if let Some(rel_end) = result[start..].find("/>") {
            let end = start + rel_end + 2;
            let block = &result[start..end];
            let replacement = extract_image_info(block);
            result = format!("{}{}{}", &result[..start], replacement, &result[end..]);
        } else {
            break;
        }
        iterations += 1;
    }
    result
}

fn extract_image_info(block: &str) -> String {
    // Try ri:filename
    if let Some(start) = block.find("ri:filename=\"") {
        let rest = &block[start + 13..];
        if let Some(end) = rest.find('"') {
            let filename = &rest[..end];
            return format!("[Image: {}]", filename);
        }
    }

    // Try ri:url
    if let Some(start) = block.find("ri:value=\"") {
        let rest = &block[start + 10..];
        if let Some(end) = rest.find('"') {
            let url = &rest[..end];
            return format!("![Image]({})", url);
        }
    }

    // Try ac:alt
    if let Some(start) = block.find("ac:alt=\"") {
        let rest = &block[start + 8..];
        if let Some(end) = rest.find('"') {
            let alt = &rest[..end];
            if !alt.is_empty() {
                return format!("[Image: {}]", alt);
            }
        }
    }

    "[Image]".into()
}

fn process_links(html: &str) -> String {
    let mut result = html.to_string();
    let mut iterations = 0;
    const MAX_ITERATIONS: usize = 1000;

    while iterations < MAX_ITERATIONS {
        let Some(start) = result.find("<ac:link") else {
            break;
        };

        if let Some(rel_end) = result[start..].find("</ac:link>") {
            let end = start + rel_end + 10;
            let block = &result[start..end];
            let replacement = extract_link_info(block);
            result = format!("{}{}{}", &result[..start], replacement, &result[end..]);
        } else if let Some(rel_end) = result[start..].find("/>") {
            let end = start + rel_end + 2;
            result = format!("{}{}", &result[..start], &result[end..]);
        } else {
            break;
        }
        iterations += 1;
    }
    result
}

fn extract_link_info(block: &str) -> String {
    // Get display text
    let display_text = extract_link_body(block)
        .or_else(|| extract_plain_text_body(block))
        .or_else(|| extract_ri_content_title(block));

    // Get target
    let target = extract_link_target(block);

    match (display_text, target) {
        (Some(text), Some(url)) => format!("[{}]({})", text.trim(), url),
        (Some(text), None) => text.trim().to_string(),
        (None, Some(url)) => format!("[{}]({})", url, url),
        (None, None) => String::new(),
    }
}

fn extract_link_body(block: &str) -> Option<String> {
    let start = block.find("<ac:link-body>")? + 14;
    let rest = &block[start..];
    let end = rest.find("</ac:link-body>")?;
    let content = &rest[..end];

    // Strip HTML tags for simple display
    let text = strip_html_tags(content);
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

fn extract_plain_text_body(block: &str) -> Option<String> {
    let start = block.find("<ac:plain-text-link-body>")? + 25;
    let rest = &block[start..];
    let end = rest.find("</ac:plain-text-link-body>")?;
    let text = rest[..end]
        .replace("<![CDATA[", "")
        .replace("]]>", "")
        .trim()
        .to_string();

    if text.is_empty() { None } else { Some(text) }
}

fn extract_ri_content_title(block: &str) -> Option<String> {
    let start = block.find("ri:content-title=\"")? + 18;
    let rest = &block[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn extract_link_target(block: &str) -> Option<String> {
    // ri:page
    if block.contains("<ri:page") {
        let title = extract_ri_content_title(block)?;
        let space = extract_attr(block, "ri:space-key");
        return match space {
            Some(s) => Some(format!("page:{}/{}", s, title)),
            None => Some(format!("page:{}", title)),
        };
    }

    // ri:user
    if block.contains("<ri:user") {
        let id =
            extract_attr(block, "ri:account-id").or_else(|| extract_attr(block, "ri:userkey"))?;
        return Some(format!("@{}", id));
    }

    // ri:attachment
    if block.contains("<ri:attachment") {
        let filename = extract_attr(block, "ri:filename")?;
        return Some(format!("attachment:{}", filename));
    }

    // ri:url
    if block.contains("<ri:url") {
        return extract_attr(block, "ri:value");
    }

    // ac:anchor
    if let Some(anchor) = extract_attr(block, "ac:anchor") {
        return Some(format!("#{}", anchor));
    }

    None
}

fn extract_attr(block: &str, attr: &str) -> Option<String> {
    let pattern = format!("{}=\"", attr);
    let start = block.find(&pattern)? + pattern.len();
    let rest = &block[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn strip_html_tags(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;

    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }

    result
}

fn process_macros(html: &str) -> String {
    let mut result = html.to_string();
    let mut iterations = 0;
    const MAX_ITERATIONS: usize = 1000;

    while iterations < MAX_ITERATIONS {
        let Some(start) = result.find("<ac:structured-macro") else {
            break;
        };

        let rest = &result[start..];

        // Check for self-closing first
        if let Some(self_close) = find_self_closing_end(rest) {
            let tag = &rest[..self_close + 2];
            let replacement = process_macro(tag);
            result = format!(
                "{}{}{}",
                &result[..start],
                replacement,
                &result[start + self_close + 2..]
            );
        }
        // Then check for paired tags
        else if let Some(rel_end) = rest.find("</ac:structured-macro>") {
            let end = start + rel_end + 22;
            let block = &result[start..end];
            let replacement = process_macro(block);
            result = format!("{}{}{}", &result[..start], replacement, &result[end..]);
        }
        // Skip unprocessable
        else {
            result = format!("{}{}", &result[..start], &result[start + 20..]);
        }

        iterations += 1;
    }
    result
}

fn find_self_closing_end(s: &str) -> Option<usize> {
    // Look for /> before finding </ac:structured-macro>
    let close_tag = s.find("</ac:structured-macro>");
    let self_close = s.find("/>");

    match (self_close, close_tag) {
        (Some(sc), Some(ct)) if sc < ct => Some(sc),
        (Some(sc), None) => Some(sc),
        _ => None,
    }
}

fn process_tasks(html: &str) -> String {
    let mut result = html.to_string();
    let mut iterations = 0;
    const MAX_ITERATIONS: usize = 1000;

    while iterations < MAX_ITERATIONS {
        let Some(start) = result.find("<ac:task-list>") else {
            break;
        };

        if let Some(rel_end) = result[start..].find("</ac:task-list>") {
            let end = start + rel_end + 15;
            let block = &result[start..end];
            let replacement = process_task_list(block);
            result = format!("{}{}{}", &result[..start], replacement, &result[end..]);
        } else {
            break;
        }
        iterations += 1;
    }
    result
}

fn process_task_list(block: &str) -> String {
    let mut tasks = Vec::new();
    let mut search_start = 0;

    while let Some(task_start) = block[search_start..].find("<ac:task>") {
        let abs_start = search_start + task_start;
        if let Some(rel_end) = block[abs_start..].find("</ac:task>") {
            let task_block = &block[abs_start..abs_start + rel_end + 10];
            tasks.push(process_task(task_block));
            search_start = abs_start + rel_end + 10;
        } else {
            break;
        }
    }

    tasks.join("\n")
}

fn process_task(task: &str) -> String {
    let status = if task.contains("<ac:task-status>complete</ac:task-status>") {
        "[x]"
    } else {
        "[ ]"
    };

    let body = if let Some(start) = task.find("<ac:task-body>") {
        let rest = &task[start + 14..];
        if let Some(end) = rest.find("</ac:task-body>") {
            strip_html_tags(&rest[..end]).trim().to_string()
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    format!("- {} {}", status, body)
}

fn process_adf_extensions(html: &str) -> String {
    let mut result = html.to_string();
    let mut iterations = 0;
    const MAX_ITERATIONS: usize = 1000;

    while iterations < MAX_ITERATIONS {
        let Some(start) = result.find("<ac:adf-extension>") else {
            break;
        };

        if let Some(rel_end) = result[start..].find("</ac:adf-extension>") {
            let end = start + rel_end + 19;
            let block = &result[start..end];
            let replacement = process_adf_extension(block);
            result = format!("{}{}{}", &result[..start], replacement, &result[end..]);
        } else {
            break;
        }
        iterations += 1;
    }
    result
}

fn process_adf_extension(block: &str) -> String {
    // Check if this is an extension type (e.g., draw.io diagram)
    if block.contains("type=\"extension\"") || block.contains("extension-type") {
        // Extract diagram name if available
        if let Some(name) = extract_adf_param_value(block, "diagram-display-name")
            .or_else(|| extract_adf_param_value(block, "diagramDisplayName"))
            .or_else(|| extract_adf_param_value(block, "diagram-name"))
        {
            return format!("[Draw.io: {}]", name);
        }
        // Check for extension title
        if let Some(title) = extract_adf_attribute(block, "extension-title") {
            return format!("[{}]", title);
        }
        return "[Embedded Diagram]".into();
    }

    // Get panel type from adf-attribute
    let panel_type = extract_adf_attribute(block, "panel-type");

    // Try adf-content first
    if let Some(content) = extract_adf_content(block) {
        let text = strip_html_tags(&content).trim().to_string();
        if !text.is_empty() {
            return match panel_type.as_deref() {
                Some("note") => format!("> **NOTE**: {}", text),
                Some("info") => format!("> **INFO**: {}", text),
                Some("warning") => format!("> **WARNING**: {}", text),
                Some("error") => format!("> **ERROR**: {}", text),
                Some("success") => format!("> **SUCCESS**: {}", text),
                _ => text,
            };
        }
    }

    // Fallback to adf-fallback
    if let Some(fallback) = extract_adf_fallback(block) {
        return strip_html_tags(&fallback).trim().to_string();
    }

    String::new()
}

fn extract_adf_param_value(block: &str, key: &str) -> Option<String> {
    // Look for patterns like: <ac:adf-parameter key="diagram-name"><ac:adf-parameter key="value">NAME</ac:adf-parameter>
    let key_pattern = format!("key=\"{}\"", key);
    let key_pos = block.find(&key_pattern)?;

    // Find the value parameter after this key
    let after_key = &block[key_pos..];
    let value_start = after_key.find("key=\"value\"")? + 11;
    let after_value = &after_key[value_start..];

    // Find the > that closes the value tag
    let content_start = after_value.find('>')? + 1;
    let content = &after_value[content_start..];

    // Find the closing </ac:adf-parameter>
    let content_end = content.find("</ac:adf-parameter>")?;
    let value = content[..content_end].trim();

    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn extract_adf_attribute(block: &str, key: &str) -> Option<String> {
    let pattern = format!("key=\"{}\"", key);
    let attr_start = block.find(&pattern)?;

    // Find the closing > of this tag
    let tag_end = block[attr_start..].find('>')?;
    let content_start = attr_start + tag_end + 1;

    // Find the closing </ac:adf-attribute>
    let close_tag = "</ac:adf-attribute>";
    let close_start = block[content_start..].find(close_tag)?;

    Some(block[content_start..content_start + close_start].to_string())
}

fn extract_adf_content(block: &str) -> Option<String> {
    let start = block.find("<ac:adf-content>")? + 16;
    let rest = &block[start..];
    let end = rest.find("</ac:adf-content>")?;
    Some(rest[..end].to_string())
}

fn extract_adf_fallback(block: &str) -> Option<String> {
    let start = block.find("<ac:adf-fallback>")? + 17;
    let rest = &block[start..];
    let end = rest.find("</ac:adf-fallback>")?;
    Some(rest[..end].to_string())
}

fn remove_remaining_tags(html: &str) -> String {
    let mut result = html.to_string();

    // First, remove paired tags with their content (like ac:parameter)
    let paired_tags = ["ac:parameter", "ac:adf-parameter", "ac:adf-parameter-value"];
    for tag in paired_tags {
        let mut iterations = 0;
        const MAX_ITERATIONS: usize = 1000;

        while iterations < MAX_ITERATIONS {
            let open_tag = format!("<{}", tag);
            let close_tag = format!("</{}>", tag);

            let Some(start) = result.find(&open_tag) else {
                break;
            };

            // Find the closing tag
            if let Some(rel_close) = result[start..].find(&close_tag) {
                let end = start + rel_close + close_tag.len();
                result = format!("{}{}", &result[..start], &result[end..]);
            } else {
                // Self-closing or malformed - just remove the opening tag
                if let Some(rel_end) = result[start..].find('>') {
                    let end = start + rel_end + 1;
                    result = format!("{}{}", &result[..start], &result[end..]);
                } else {
                    break;
                }
            }
            iterations += 1;
        }
    }

    // Then remove remaining unpaired tags
    let patterns = ["<ac:", "<ri:", "</ac:", "</ri:"];
    for pattern in patterns {
        let mut iterations = 0;
        const MAX_ITERATIONS: usize = 1000;

        while iterations < MAX_ITERATIONS {
            let Some(start) = result.find(pattern) else {
                break;
            };

            if let Some(rel_end) = result[start..].find('>') {
                let end = start + rel_end + 1;
                result = format!("{}{}", &result[..start], &result[end..]);
            } else {
                break;
            }
            iterations += 1;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emoticon() {
        let html = r#"<ac:emoticon ac:name="smile" />"#;
        assert_eq!(clean_confluence_html(html), ":)");
    }

    #[test]
    fn test_image_with_filename() {
        let html = r#"<ac:image><ri:attachment ri:filename="diagram.png"/></ac:image>"#;
        assert_eq!(clean_confluence_html(html), "[Image: diagram.png]");
    }

    #[test]
    fn test_image_with_url() {
        let html = r#"<ac:image><ri:url ri:value="https://example.com/img.png"/></ac:image>"#;
        assert_eq!(
            clean_confluence_html(html),
            "![Image](https://example.com/img.png)"
        );
    }

    #[test]
    fn test_link_with_body() {
        let html = r#"<ac:link><ri:page ri:content-title="My Page"/><ac:link-body>Click here</ac:link-body></ac:link>"#;
        assert_eq!(clean_confluence_html(html), "[Click here](page:My Page)");
    }

    #[test]
    fn test_link_with_plain_text() {
        let html = r#"<ac:link><ri:page ri:content-title="My Page"/><ac:plain-text-link-body><![CDATA[Link text]]></ac:plain-text-link-body></ac:link>"#;
        assert_eq!(clean_confluence_html(html), "[Link text](page:My Page)");
    }

    #[test]
    fn test_link_to_user() {
        let html = r#"<ac:link><ri:user ri:account-id="user123"/></ac:link>"#;
        assert_eq!(clean_confluence_html(html), "[@user123](@user123)");
    }

    #[test]
    fn test_self_closing_macro() {
        let html = r#"<ac:structured-macro ac:name="toc" /><p>After</p>"#;
        let result = clean_confluence_html(html);
        assert!(result.contains("<p>After</p>"));
    }

    #[test]
    fn test_task_list() {
        let html = r#"<ac:task-list>
            <ac:task><ac:task-status>incomplete</ac:task-status><ac:task-body>Todo item</ac:task-body></ac:task>
            <ac:task><ac:task-status>complete</ac:task-status><ac:task-body>Done item</ac:task-body></ac:task>
        </ac:task-list>"#;
        let result = clean_confluence_html(html);
        assert!(result.contains("- [ ] Todo item"));
        assert!(result.contains("- [x] Done item"));
    }

    #[test]
    fn test_adf_extension_panel() {
        let html = r#"<ac:adf-extension><ac:adf-node type="panel"><ac:adf-attribute key="panel-type">note</ac:adf-attribute><ac:adf-content><p>Content</p></ac:adf-content></ac:adf-node></ac:adf-extension>"#;
        let result = clean_confluence_html(html);
        assert!(result.contains("> **NOTE**"));
        assert!(result.contains("Content"));
        assert!(!result.contains("panel-type"));
    }

    #[test]
    fn test_remove_remaining_tags() {
        let html = r#"<ac:unknown>text</ac:unknown><ri:unknown />"#;
        let result = clean_confluence_html(html);
        assert_eq!(result, "text");
    }
}
