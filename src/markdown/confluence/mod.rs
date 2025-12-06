mod cleanup;
mod elements;
mod macros;

use crate::markdown::common::normalize_whitespace;
use htmd::HtmlToMarkdown;

pub fn confluence_to_markdown(html: &str) -> String {
    // 1. Pre-process: Process Confluence elements first (needs full metadata)
    let processed = elements::clean_confluence_html(html);

    // 2. Clean metadata after Confluence element processing
    let cleaned = cleanup::clean_metadata(&processed);

    // 3. Convert standard HTML to Markdown
    let converter = HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style", "meta", "noscript"])
        .build();

    let markdown = converter
        .convert(&cleaned)
        .unwrap_or_else(|_| cleaned.clone());

    // 4. Post-process: Remove binary residue, unescape, and normalize
    let without_residue = cleanup::clean_binary_data(&markdown);
    let unescaped = unescape_markdown(&without_residue);
    normalize_whitespace(&unescaped)
}

fn unescape_markdown(text: &str) -> String {
    text.replace("\\[", "[")
        .replace("\\]", "]")
        .replace("\\*", "*")
        .replace("\\_", "_")
        .replace("\\`", "`")
        .replace("\\#", "#")
        .replace("\\>", ">")
        .replace("\\-", "-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_html() {
        let html = "<p>Hello <strong>world</strong></p>";
        let result = confluence_to_markdown(html);
        assert!(result.contains("**world**"));
    }

    #[test]
    fn test_headings() {
        let html = "<h1>Title</h1><h2>Subtitle</h2>";
        let result = confluence_to_markdown(html);
        assert!(result.contains("# Title"));
        assert!(result.contains("## Subtitle"));
    }

    #[test]
    fn test_lists() {
        let html = "<ul><li>Item 1</li><li>Item 2</li></ul>";
        let result = confluence_to_markdown(html);
        assert!(result.contains("Item 1"));
        assert!(result.contains("Item 2"));
    }

    #[test]
    fn test_links() {
        let html = r#"<a href="https://example.com">Example</a>"#;
        let result = confluence_to_markdown(html);
        assert!(result.contains("[Example]"));
        assert!(result.contains("https://example.com"));
    }

    #[test]
    fn test_code_macro() {
        let html = r#"<ac:structured-macro ac:name="code" ac:macro-id="123">
            <ac:parameter ac:name="language">rust</ac:parameter>
            <ac:plain-text-body><![CDATA[let x = 1;]]></ac:plain-text-body>
        </ac:structured-macro>"#;
        let result = confluence_to_markdown(html);
        assert!(result.contains("```rust"));
        assert!(result.contains("let x = 1;"));
    }

    #[test]
    fn test_info_panel() {
        let html = r#"<ac:structured-macro ac:name="info">
            <ac:rich-text-body><p>Important note</p></ac:rich-text-body>
        </ac:structured-macro>"#;
        let result = confluence_to_markdown(html);
        assert!(result.contains("INFO"));
        assert!(result.contains("Important note"));
    }

    #[test]
    fn test_confluence_image() {
        let html = r#"<ac:image><ri:attachment ri:filename="diagram.png"/></ac:image>"#;
        let result = confluence_to_markdown(html);
        assert!(result.contains("[Image: diagram.png]"));
    }

    #[test]
    fn test_drawio() {
        let html = r#"<ac:structured-macro ac:name="drawio" ac:macro-id="uuid">
            <ac:parameter ac:name="diagramName">architecture</ac:parameter>
            <ac:parameter ac:name="contentId">123</ac:parameter>
            <ac:parameter ac:name="pageId">456</ac:parameter>
        </ac:structured-macro>"#;
        let result = confluence_to_markdown(html);
        assert_eq!(result, "[Draw.io: architecture]");
    }

    #[test]
    fn test_task_list() {
        let html = r#"<ac:task-list>
            <ac:task><ac:task-status>incomplete</ac:task-status><ac:task-body>Todo</ac:task-body></ac:task>
            <ac:task><ac:task-status>complete</ac:task-status><ac:task-body>Done</ac:task-body></ac:task>
        </ac:task-list>"#;
        let result = confluence_to_markdown(html);
        assert!(result.contains("- [ ] Todo"));
        assert!(result.contains("- [x] Done"));
    }

    #[test]
    fn test_adf_extension() {
        let html = r#"<ac:adf-extension><ac:adf-node type="panel"><ac:adf-attribute key="panel-type">note</ac:adf-attribute><ac:adf-content><p>Content</p></ac:adf-content></ac:adf-node></ac:adf-extension>"#;
        let result = confluence_to_markdown(html);
        assert!(result.contains("> **NOTE**"));
        assert!(result.contains("Content"));
    }

    #[test]
    fn test_user_mention() {
        let html = r#"Contact <ac:link><ri:user ri:account-id="user123"/></ac:link> for help."#;
        let result = confluence_to_markdown(html);
        assert!(result.contains("@user123"));
    }

    #[test]
    fn test_self_closing_toc() {
        let html = r#"<ac:structured-macro ac:name="toc" /><p>Content after TOC</p>"#;
        let result = confluence_to_markdown(html);
        assert!(result.contains("Content after TOC"));
    }

    #[test]
    fn test_status_macro() {
        let html = r#"<ac:structured-macro ac:name="status">
            <ac:parameter ac:name="title">Done</ac:parameter>
            <ac:parameter ac:name="colour">Green</ac:parameter>
        </ac:structured-macro>"#;
        let result = confluence_to_markdown(html);
        assert!(result.contains("[OK] DONE"));
    }

    #[test]
    fn test_expand_macro() {
        let html = r#"<ac:structured-macro ac:name="expand">
            <ac:parameter ac:name="title">Show more</ac:parameter>
            <ac:rich-text-body><p>Hidden content</p></ac:rich-text-body>
        </ac:structured-macro>"#;
        let result = confluence_to_markdown(html);
        assert!(result.contains("**Show more**"));
        assert!(result.contains("Hidden content"));
    }

    #[test]
    fn test_emoticon() {
        let html = r#"<ac:emoticon ac:name="smile" /> Hello!"#;
        let result = confluence_to_markdown(html);
        assert!(result.contains(":)"));
        assert!(result.contains("Hello!"));
    }

    #[test]
    fn test_complex_link() {
        let html = r#"<ac:link><ri:page ri:space-key="PROJ" ri:content-title="My Page"/><ac:link-body><strong>Click here</strong></ac:link-body></ac:link>"#;
        let result = confluence_to_markdown(html);
        assert!(result.contains("[Click here](page:PROJ/My Page)"));
    }

    #[test]
    fn test_table() {
        let html = r#"<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>"#;
        let result = confluence_to_markdown(html);
        assert!(result.contains("|"));
        assert!(result.contains("A"));
        assert!(result.contains("B"));
    }

    #[test]
    fn test_removes_metadata() {
        let html = r#"<ac:structured-macro ac:name="code" ac:macro-id="uuid-123" ac:schema-version="1">
            <ac:plain-text-body>code</ac:plain-text-body>
        </ac:structured-macro>"#;
        let result = confluence_to_markdown(html);
        assert!(!result.contains("uuid-123"));
        assert!(!result.contains("schema-version"));
    }

    #[test]
    fn test_removes_long_base64_residue() {
        // Simulates macro residue: 500+ continuous characters without spaces in table cell
        let base64_residue = "A".repeat(600);
        let html = format!(
            r#"<table><tr><td>Header</td></tr><tr><td>Data {}</td></tr></table>"#,
            base64_residue
        );
        let result = confluence_to_markdown(&html);
        assert!(result.contains("Header"));
        assert!(result.contains("Data"));
        assert!(
            !result.contains(&base64_residue),
            "500+ char residue should be removed"
        );
    }

    #[test]
    fn test_removes_mxgraphmodel_residue() {
        let html = r#"<p>Before</p><mxGraphModel><root><mxCell id="0"/></root></mxGraphModel><p>After</p>"#;
        let result = confluence_to_markdown(html);
        assert!(result.contains("Before"));
        assert!(result.contains("After"));
        assert!(!result.contains("mxGraphModel"));
        assert!(!result.contains("mxCell"));
    }
}
