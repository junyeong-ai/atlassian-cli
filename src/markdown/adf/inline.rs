use super::marks::apply_marks;
use serde_json::Value;

pub fn convert_inline_nodes(nodes: &[Value]) -> String {
    nodes.iter().filter_map(convert_inline_node).collect()
}

/// The Markdown for an inline node, or `None` for a node that is not inline.
pub fn convert_inline_node(node: &Value) -> Option<String> {
    let node_type = node.get("type").and_then(|t| t.as_str()).unwrap_or("");

    Some(match node_type {
        "text" => convert_text(node),
        "hardBreak" => "\\\n".into(),
        "mention" => convert_mention(node),
        "emoji" => convert_emoji(node),
        "inlineCard" => convert_inline_card(node),
        "date" => convert_date(node),
        "status" => convert_status(node),
        "mediaInline" => convert_media_inline(node),
        "placeholder" => convert_placeholder(node),
        _ => return None,
    })
}

fn convert_text(node: &Value) -> String {
    let text = node
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();

    let marks = node.get("marks").and_then(|m| m.as_array());
    apply_marks(text, marks)
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
    let shortname = node
        .get("attrs")
        .and_then(|a| a.get("shortName"))
        .and_then(|s| s.as_str());

    let text = node
        .get("attrs")
        .and_then(|a| a.get("text"))
        .and_then(|t| t.as_str());

    text.or(shortname).unwrap_or("").to_string()
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

fn convert_date(node: &Value) -> String {
    let timestamp = node
        .get("attrs")
        .and_then(|a| a.get("timestamp"))
        .and_then(|t| t.as_str())
        .unwrap_or("");

    if timestamp.is_empty() {
        return String::new();
    }

    if let Ok(ts) = timestamp.parse::<i64>() {
        let secs = ts / 1000;
        let date = chrono_format_date(secs);
        return date;
    }

    timestamp.to_string()
}

fn chrono_format_date(secs: i64) -> String {
    // Handle negative timestamps (pre-1970) by returning raw timestamp
    if secs < 0 {
        return format!("1970-01-01 (pre-epoch: {})", secs);
    }

    // Validate reasonable date range (1970 to 3000)
    const MAX_SECS: i64 = 32503680000; // Year 3000 approx
    if secs > MAX_SECS {
        return format!("(invalid timestamp: {})", secs);
    }

    let days_since_epoch = secs / 86400;
    let mut year = 1970i32;
    let mut remaining_days = days_since_epoch;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }

    let days_in_months: [i64; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1u32;
    for days in days_in_months {
        if remaining_days < days {
            break;
        }
        remaining_days -= days;
        month += 1;
    }

    let day = remaining_days + 1;
    format!("{:04}-{:02}-{:02}", year, month, day)
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn convert_status(node: &Value) -> String {
    let attrs = node.get("attrs");
    let text = attrs
        .and_then(|a| a.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("status");
    let color = attrs
        .and_then(|a| a.get("color"))
        .and_then(|c| c.as_str())
        .unwrap_or("neutral");

    let indicator = match color {
        "green" => "[OK]",
        "yellow" => "[WARN]",
        "red" => "[ERR]",
        "blue" => "[INFO]",
        "purple" => "[NOTE]",
        _ => "[STATUS]",
    };

    format!("{} {}", indicator, text.to_uppercase())
}

fn convert_media_inline(node: &Value) -> String {
    let attrs = node.get("attrs");
    let alt = attrs
        .and_then(|a| a.get("alt"))
        .and_then(|a| a.as_str())
        .or_else(|| attrs.and_then(|a| a.get("id")).and_then(|i| i.as_str()))
        .unwrap_or("media");

    format!("[Media: {}]", alt)
}

fn convert_placeholder(node: &Value) -> String {
    let text = node
        .get("attrs")
        .and_then(|a| a.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("placeholder");

    format!("{{{}}}", text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_text() {
        let node = json!({"type": "text", "text": "hello"});
        assert_eq!(convert_inline_node(&node).unwrap(), "hello");
    }

    #[test]
    fn test_text_with_marks() {
        let node = json!({
            "type": "text",
            "text": "bold",
            "marks": [{"type": "strong"}]
        });
        assert_eq!(convert_inline_node(&node).unwrap(), "**bold**");
    }

    #[test]
    fn test_hard_break() {
        let node = json!({"type": "hardBreak"});
        assert_eq!(convert_inline_node(&node).unwrap(), "\\\n");
    }

    #[test]
    fn test_mention() {
        let node = json!({"type": "mention", "attrs": {"text": "@john"}});
        assert_eq!(convert_inline_node(&node).unwrap(), "@john");
    }

    #[test]
    fn test_mention_with_id() {
        let node = json!({"type": "mention", "attrs": {"id": "user123"}});
        assert_eq!(convert_inline_node(&node).unwrap(), "@user123");
    }

    #[test]
    fn test_emoji() {
        let node = json!({"type": "emoji", "attrs": {"shortName": ":smile:", "text": "😄"}});
        assert_eq!(convert_inline_node(&node).unwrap(), "😄");
    }

    #[test]
    fn test_inline_card() {
        let node = json!({"type": "inlineCard", "attrs": {"url": "https://example.com"}});
        assert_eq!(
            convert_inline_node(&node).unwrap(),
            "[https://example.com](https://example.com)"
        );
    }

    #[test]
    fn test_date() {
        let node = json!({"type": "date", "attrs": {"timestamp": "1704067200000"}}); // 2024-01-01
        assert_eq!(convert_inline_node(&node).unwrap(), "2024-01-01");
    }

    #[test]
    fn test_status_green() {
        let node = json!({"type": "status", "attrs": {"text": "done", "color": "green"}});
        assert_eq!(convert_inline_node(&node).unwrap(), "[OK] DONE");
    }

    #[test]
    fn test_status_red() {
        let node = json!({"type": "status", "attrs": {"text": "failed", "color": "red"}});
        assert_eq!(convert_inline_node(&node).unwrap(), "[ERR] FAILED");
    }

    #[test]
    fn test_media_inline() {
        let node = json!({"type": "mediaInline", "attrs": {"alt": "screenshot.png"}});
        assert_eq!(
            convert_inline_node(&node).unwrap(),
            "[Media: screenshot.png]"
        );
    }

    #[test]
    fn test_placeholder() {
        let node = json!({"type": "placeholder", "attrs": {"text": "Enter name"}});
        assert_eq!(convert_inline_node(&node).unwrap(), "{Enter name}");
    }

    #[test]
    fn test_multiple_inline_nodes() {
        let nodes = vec![
            json!({"type": "text", "text": "Hello "}),
            json!({"type": "text", "text": "world", "marks": [{"type": "strong"}]}),
        ];
        assert_eq!(convert_inline_nodes(&nodes), "Hello **world**");
    }
}
