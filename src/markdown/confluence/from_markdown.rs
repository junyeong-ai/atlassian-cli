use crate::markdown::common::parse_markdown;
use percent_encoding::percent_decode_str;
use pulldown_cmark::{CodeBlockKind, Event, Tag, TagEnd, html};

/// Convert Markdown to Confluence storage format.
///
/// Storage format is XHTML, so the standard HTML rendering carries over with
/// three substitutions: a code block becomes the `code` macro, strikethrough the
/// span storage format documents for it, and an image `ac:image` — a URL when
/// the source has a scheme, otherwise an attachment of the page by that name.
/// Raw HTML passes through as CommonMark specifies; its tag names cannot carry
/// a namespace, so storage-format macros (`ac:*`) need `--format storage`.
pub fn markdown_to_confluence(markdown: &str) -> String {
    let mut events = Vec::new();
    let mut code: Option<(Option<String>, String)> = None;
    let mut image: Option<(String, String)> = None;

    for event in parse_markdown(markdown) {
        if let Some((language, body)) = code.as_mut() {
            match event {
                Event::Text(text) => body.push_str(&text),
                Event::End(TagEnd::CodeBlock) => {
                    events.push(Event::Html(code_macro(language.as_deref(), body).into()));
                    code = None;
                }
                _ => {}
            }
            continue;
        }
        if let Some((source, alt)) = image.as_mut() {
            match event {
                Event::Text(text) | Event::Code(text) | Event::InlineHtml(text) => {
                    alt.push_str(&text)
                }
                Event::SoftBreak | Event::HardBreak => alt.push(' '),
                Event::End(TagEnd::Image) => {
                    events.push(Event::InlineHtml(image_macro(source, alt).into()));
                    image = None;
                }
                _ => {}
            }
            continue;
        }
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().map(str::to_string)
                    }
                    CodeBlockKind::Indented => None,
                };
                code = Some((language, String::new()));
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                image = Some((dest_url.into_string(), String::new()));
            }
            Event::Start(Tag::Strikethrough) => events.push(Event::InlineHtml(
                r#"<span style="text-decoration: line-through;">"#.into(),
            )),
            Event::End(TagEnd::Strikethrough) => events.push(Event::InlineHtml("</span>".into())),
            other => events.push(other),
        }
    }

    let mut storage = String::new();
    html::push_html(&mut storage, events.into_iter());
    storage
}

fn code_macro(language: Option<&str>, body: &str) -> String {
    let mut code = String::from(r#"<ac:structured-macro ac:name="code">"#);
    if let Some(language) = language {
        code.push_str(&format!(
            r#"<ac:parameter ac:name="language">{}</ac:parameter>"#,
            escape(language)
        ));
    }
    // A CDATA section ends at the first `]]>`, so one inside the code is split
    // across two sections.
    let body = body
        .trim_end_matches('\n')
        .replace("]]>", "]]]]><![CDATA[>");
    code.push_str(&format!(
        "<ac:plain-text-body><![CDATA[{body}]]></ac:plain-text-body></ac:structured-macro>\n"
    ));
    code
}

fn image_macro(source: &str, alt: &str) -> String {
    let resource = if has_scheme(source) {
        format!(r#"<ri:url ri:value="{}" />"#, escape(source))
    } else {
        let filename = percent_decode_str(source).decode_utf8_lossy();
        format!(r#"<ri:attachment ri:filename="{}" />"#, escape(&filename))
    };
    format!(
        r#"<ac:image ac:alt="{}">{resource}</ac:image>"#,
        escape(alt)
    )
}

/// Whether a reference starts with an RFC 3986 scheme. A relative reference
/// cannot: its first segment may not contain `:`.
fn has_scheme(reference: &str) -> bool {
    reference.split_once(':').is_some_and(|(scheme, _)| {
        let mut chars = scheme.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_blocks_become_the_code_macro() {
        assert_eq!(
            markdown_to_confluence("```rust\nlet end = \"]]>\";\n```\n\n    plain\n"),
            "<ac:structured-macro ac:name=\"code\">\
             <ac:parameter ac:name=\"language\">rust</ac:parameter>\
             <ac:plain-text-body><![CDATA[let end = \"]]]]><![CDATA[>\";]]></ac:plain-text-body>\
             </ac:structured-macro>\n\
             <ac:structured-macro ac:name=\"code\">\
             <ac:plain-text-body><![CDATA[plain]]></ac:plain-text-body></ac:structured-macro>\n"
        );
    }

    #[test]
    fn images_reference_a_url_or_an_attachment_by_scheme() {
        assert_eq!(
            markdown_to_confluence(
                "![a \"chart\"](https://img.example/c.png?x=1&y=2) ![](<Screen Shot.png>) ![](my%20diagram.png)"
            ),
            "<p><ac:image ac:alt=\"a &quot;chart&quot;\">\
             <ri:url ri:value=\"https://img.example/c.png?x=1&amp;y=2\" /></ac:image> \
             <ac:image ac:alt=\"\"><ri:attachment ri:filename=\"Screen Shot.png\" /></ac:image> \
             <ac:image ac:alt=\"\"><ri:attachment ri:filename=\"my diagram.png\" /></ac:image></p>\n"
        );
    }

    #[test]
    fn markup_renders_as_storage_xhtml() {
        assert_eq!(
            markdown_to_confluence(
                "## Plan\n\n~~old~~ **new** & <u>raw</u> <ac:emoticon />\n\n\
                 3. step\n\n| A | B |\n|---|---|\n| 1 | 2 |\n"
            ),
            "<h2>Plan</h2>\n\
             <p><span style=\"text-decoration: line-through;\">old</span> <strong>new</strong> &amp; \
             <u>raw</u> &lt;ac:emoticon /&gt;</p>\n\
             <ol start=\"3\">\n<li>step</li>\n</ol>\n\
             <table><thead><tr><th>A</th><th>B</th></tr></thead><tbody>\n\
             <tr><td>1</td><td>2</td></tr>\n</tbody></table>\n"
        );
    }
}
