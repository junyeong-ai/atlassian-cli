use crate::markdown::common::parse_markdown;
use anyhow::{Result, bail};
use percent_encoding::percent_decode_str;
use pulldown_cmark::{CodeBlockKind, CowStr, Event, Tag, TagEnd, html};

/// Convert Markdown to Confluence storage format.
///
/// Storage format is XHTML, so the standard HTML rendering carries over with
/// three substitutions: a code block becomes the `code` macro, strikethrough the
/// span storage format documents for it, and an image `ac:image` — a URL when
/// the source has a scheme, otherwise the page attachment named by its last path
/// segment, the name `attachment upload` gives a file. Raw HTML stays literal
/// text, as it does in ADF: HTML need not be well-formed XML, and Markdown tag
/// names cannot carry the `ac:` namespace, so macros need `--format storage`.
/// Characters XML cannot carry are refused, which keeps the output well-formed.
pub fn markdown_to_confluence(markdown: &str) -> Result<String> {
    let mut events = Vec::new();
    let mut code: Option<(Option<String>, String)> = None;
    let mut html_block: Option<String> = None;
    let mut image: Option<Image> = None;

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
        if let Some(block) = html_block.as_mut() {
            match event {
                Event::Html(text) => block.push_str(&text),
                Event::End(TagEnd::HtmlBlock) => {
                    events.push(Event::Start(Tag::Paragraph));
                    for (index, line) in block.lines().filter(|l| !l.is_empty()).enumerate() {
                        if index > 0 {
                            events.push(Event::HardBreak);
                        }
                        events.push(Event::Text(CowStr::from(line.to_string())));
                    }
                    events.push(Event::End(TagEnd::Paragraph));
                    html_block = None;
                }
                _ => {}
            }
            continue;
        }
        if let Some(open) = image.as_mut() {
            match event {
                Event::Text(text) | Event::Code(text) | Event::InlineHtml(text) => {
                    open.alt.push_str(&text)
                }
                Event::SoftBreak | Event::HardBreak => open.alt.push(' '),
                Event::Start(Tag::Image { .. }) => open.nested += 1,
                Event::End(TagEnd::Image) if open.nested > 0 => open.nested -= 1,
                Event::End(TagEnd::Image) => {
                    events.push(Event::InlineHtml(
                        image_macro(&open.source, &open.alt)?.into(),
                    ));
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
            Event::Start(Tag::HtmlBlock) => html_block = Some(String::new()),
            Event::Start(Tag::Image { dest_url, .. }) => {
                image = Some(Image {
                    source: dest_url.into_string(),
                    alt: String::new(),
                    nested: 0,
                });
            }
            Event::Start(Tag::Strikethrough) => events.push(Event::InlineHtml(
                r#"<span style="text-decoration: line-through;">"#.into(),
            )),
            Event::End(TagEnd::Strikethrough) => events.push(Event::InlineHtml("</span>".into())),
            Event::Html(text) | Event::InlineHtml(text) => events.push(Event::Text(text)),
            other => events.push(other),
        }
    }

    let mut storage = String::new();
    html::push_html(&mut storage, events.into_iter());
    // Checked on the output because Markdown decodes character references:
    // `&#1;` is clean source that renders a character XML excludes.
    if let Some(character) = storage.chars().find(|&c| !is_xml_char(c)) {
        let location = match markdown.find(character) {
            Some(offset) => format!("on line {}", markdown[..offset].matches('\n').count() + 1),
            None => "through an escape".to_string(),
        };
        bail!(
            "Markdown contains U+{:04X} {location}, which storage format cannot carry; remove it",
            u32::from(character)
        );
    }
    Ok(storage)
}

struct Image {
    source: String,
    alt: String,
    /// Images opened inside this one's alt text, whose ends are not its own.
    nested: usize,
}

/// The characters XML 1.0 admits; storage format is parsed as XML.
fn is_xml_char(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..
    )
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

fn image_macro(source: &str, alt: &str) -> Result<String> {
    let resource = if has_scheme(source) || source.starts_with("//") {
        format!(r#"<ri:url ri:value="{}" />"#, escape(source))
    } else {
        let path = source.split(['?', '#']).next().unwrap_or_default();
        let segment = path.rsplit('/').next().unwrap_or_default();
        let filename = percent_decode_str(segment).decode_utf8_lossy();
        if filename.is_empty() {
            bail!("image source {source:?} names no attachment file");
        }
        format!(r#"<ri:attachment ri:filename="{}" />"#, escape(&filename))
    };
    Ok(format!(
        r#"<ac:image ac:alt="{}">{resource}</ac:image>"#,
        escape(alt)
    ))
}

/// Whether a reference starts with an RFC 3986 scheme. A relative reference
/// cannot: its first segment may not contain `:`. A network-path reference
/// (`//host/…`) has no scheme but is still a URL, so callers test for it too.
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

    fn storage(markdown: &str) -> String {
        markdown_to_confluence(markdown).unwrap()
    }

    #[test]
    fn code_blocks_become_the_code_macro() {
        assert_eq!(
            storage("```rust\nlet end = \"]]>\";\n```\n\n    plain\n"),
            "<ac:structured-macro ac:name=\"code\">\
             <ac:parameter ac:name=\"language\">rust</ac:parameter>\
             <ac:plain-text-body><![CDATA[let end = \"]]]]><![CDATA[>\";]]></ac:plain-text-body>\
             </ac:structured-macro>\n\
             <ac:structured-macro ac:name=\"code\">\
             <ac:plain-text-body><![CDATA[plain]]></ac:plain-text-body></ac:structured-macro>\n"
        );
    }

    #[test]
    fn images_reference_a_url_or_the_attachment_upload_names() {
        assert_eq!(
            storage(
                "![a \"chart\"](https://img.example/c.png?x=1&y=2) ![](<Screen Shot.png>) \
                 ![](images/my%20diagram.png?raw=1#top) ![](a%2Fb.png) ![a ![b](c) d](e.png) \
                 ![](//cdn.example/x.png)"
            ),
            "<p><ac:image ac:alt=\"a &quot;chart&quot;\">\
             <ri:url ri:value=\"https://img.example/c.png?x=1&amp;y=2\" /></ac:image> \
             <ac:image ac:alt=\"\"><ri:attachment ri:filename=\"Screen Shot.png\" /></ac:image> \
             <ac:image ac:alt=\"\"><ri:attachment ri:filename=\"my diagram.png\" /></ac:image> \
             <ac:image ac:alt=\"\"><ri:attachment ri:filename=\"a/b.png\" /></ac:image> \
             <ac:image ac:alt=\"a b d\"><ri:attachment ri:filename=\"e.png\" /></ac:image> \
             <ac:image ac:alt=\"\"><ri:url ri:value=\"//cdn.example/x.png\" /></ac:image></p>\n"
        );
    }

    #[test]
    fn markup_renders_as_storage_xhtml_and_raw_html_stays_text() {
        assert_eq!(
            storage(
                "## Plan\n\n~~old~~ **new** & <br> <ac:emoticon />\n\n\
                 3. step\n\n| A | B |\n|---|---|\n| 1 | <br> |\n\n<div>\nkept\n</div>\n"
            ),
            "<h2>Plan</h2>\n\
             <p><span style=\"text-decoration: line-through;\">old</span> <strong>new</strong> &amp; \
             &lt;br&gt; &lt;ac:emoticon /&gt;</p>\n\
             <ol start=\"3\">\n<li>step</li>\n</ol>\n\
             <table><thead><tr><th>A</th><th>B</th></tr></thead><tbody>\n\
             <tr><td>1</td><td>&lt;br&gt;</td></tr>\n</tbody></table>\n\
             <p>&lt;div&gt;<br />\nkept<br />\n&lt;/div&gt;</p>\n"
        );
    }

    #[test]
    fn characters_xml_cannot_carry_are_refused_with_their_line() {
        let err = markdown_to_confluence("ok\n```\nx\u{1b}[31my\n```")
            .unwrap_err()
            .to_string();
        assert!(err.contains("U+001B on line 3"), "{err}");
        for reference in ["a &#1; b", "[a](u \"t&#x1B;\")", "```&#8;\nx\n```"] {
            let err = markdown_to_confluence(reference).unwrap_err().to_string();
            assert!(err.contains("through an escape"), "{reference}: {err}");
        }
        assert!(markdown_to_confluence("tab\tand\r\nnewline").is_ok());
        let err = markdown_to_confluence("![x](images/)")
            .unwrap_err()
            .to_string();
        assert!(err.contains("names no attachment file"), "{err}");
    }
}
