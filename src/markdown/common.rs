use pulldown_cmark::{Options, Parser};

/// The Markdown that writes accept: CommonMark with GitHub's tables and
/// strikethrough. Both converters parse through here so the two products agree
/// on what a document means.
pub fn parse_markdown(markdown: &str) -> Parser<'_> {
    Parser::new_ext(
        markdown,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH,
    )
}

pub fn normalize_whitespace(text: &str) -> String {
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

    // Trim leading empty lines - O(1) slice instead of O(n) remove
    let start = lines
        .iter()
        .position(|l| !l.is_empty())
        .unwrap_or(lines.len());
    let end = lines
        .iter()
        .rposition(|l| !l.is_empty())
        .map(|i| i + 1)
        .unwrap_or(0);

    if start >= end {
        return String::new();
    }

    lines[start..end].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_whitespace() {
        assert_eq!(normalize_whitespace("a\n\n\n\nb"), "a\n\nb");
        assert_eq!(normalize_whitespace("\n\na\n\n"), "a");
        assert_eq!(normalize_whitespace("a\nb\nc"), "a\nb\nc");
    }
}
