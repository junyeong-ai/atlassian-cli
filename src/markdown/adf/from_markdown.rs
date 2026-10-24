use super::MAX_DEPTH;
use crate::markdown::common::parse_markdown;
use anyhow::{Result, bail};
use pulldown_cmark::{CodeBlockKind, CowStr, Event, LinkType, Tag, TagEnd};
use serde_json::{Value, json};

/// Convert Markdown to an Atlassian Document Format document.
///
/// ADF admits fewer nestings than Markdown: a list item or a quote holds only
/// paragraphs, lists and code blocks, so Markdown nesting anything else there
/// is refused rather than reshaped. Inline code keeps only a link, the one mark
/// ADF combines with it. An image becomes its alt text linked to the image, and
/// raw HTML stays literal text.
pub fn markdown_to_adf(markdown: &str) -> Result<Value> {
    let mut builder = Builder::default();
    for event in parse_markdown(markdown) {
        builder.push(event)?;
    }
    builder.finish()
}

#[derive(Default)]
struct Builder {
    doc: Vec<Value>,
    frames: Vec<Frame>,
    marks: Vec<Mark>,
    image: Option<Image>,
}

struct Frame {
    block: Block,
    content: Vec<Value>,
}

enum Block {
    Paragraph,
    /// The paragraph a tight list item's text is implied to sit in.
    ImplicitParagraph,
    Heading(u8),
    Quote,
    List(Option<u64>),
    Item,
    Code {
        language: Option<String>,
        text: String,
    },
    Html(String),
    Table,
    Row {
        header: bool,
    },
    Cell,
}

impl Block {
    fn holds_inline(&self) -> bool {
        matches!(
            self,
            Block::Paragraph | Block::ImplicitParagraph | Block::Heading(_) | Block::Cell
        )
    }

    fn holds_only_simple_blocks(&self) -> bool {
        matches!(self, Block::Item | Block::Quote)
    }
}

enum Mark {
    Strong,
    Em,
    Strike,
    Link(String),
}

impl Mark {
    fn kind(&self) -> &'static str {
        match self {
            Mark::Strong => "strong",
            Mark::Em => "em",
            Mark::Strike => "strike",
            Mark::Link(_) => "link",
        }
    }

    fn to_value(&self) -> Value {
        match self {
            Mark::Link(href) => link_mark(href),
            other => json!({ "type": other.kind() }),
        }
    }
}

struct Image {
    source: String,
    alt: String,
    /// Images opened inside this one's alt text, whose ends are not its own.
    nested: usize,
}

impl Builder {
    fn push(&mut self, event: Event) -> Result<()> {
        if self.image.is_some() {
            self.push_alt(event);
            return Ok(());
        }
        if let Some(Frame {
            block: Block::Code { text, .. } | Block::Html(text),
            ..
        }) = self.frames.last_mut()
            && let Event::Text(chunk) | Event::Html(chunk) = &event
        {
            text.push_str(chunk);
            return Ok(());
        }
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(end) => self.end(end),
            Event::Text(text) | Event::InlineHtml(text) | Event::Html(text) => {
                let marks = self.mark_values();
                self.text(&text, marks);
                Ok(())
            }
            Event::Code(code) => {
                let mut marks = vec![json!({ "type": "code" })];
                marks.extend(
                    self.marks
                        .iter()
                        .find(|m| matches!(m, Mark::Link(_)))
                        .map(Mark::to_value),
                );
                self.text(&code, marks);
                Ok(())
            }
            Event::SoftBreak => {
                let marks = self.mark_values();
                self.text(" ", marks);
                Ok(())
            }
            Event::HardBreak => {
                self.inline_frame()
                    .content
                    .push(json!({ "type": "hardBreak" }));
                Ok(())
            }
            Event::Rule => {
                self.close_implicit()?;
                self.append(json!({ "type": "rule" }))
            }
            other => unreachable!("parse_markdown enables no extension that emits {other:?}"),
        }
    }

    fn push_alt(&mut self, event: Event) {
        let Some(image) = self.image.as_mut() else {
            return;
        };
        match event {
            Event::Text(text) | Event::Code(text) | Event::InlineHtml(text) => {
                image.alt.push_str(&text)
            }
            Event::SoftBreak | Event::HardBreak => image.alt.push(' '),
            Event::Start(Tag::Image { .. }) => image.nested += 1,
            Event::End(TagEnd::Image) if image.nested > 0 => image.nested -= 1,
            Event::End(TagEnd::Image) => {
                let Image { source, alt, .. } = self.image.take().expect("an image is open");
                let mut marks = self.mark_values();
                if !self.marks.iter().any(|m| matches!(m, Mark::Link(_))) {
                    marks.push(link_mark(&source));
                }
                let text = if alt.is_empty() { source } else { alt };
                self.text(&text, marks);
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) -> Result<()> {
        let mark = match tag {
            Tag::Strong => Mark::Strong,
            Tag::Emphasis => Mark::Em,
            Tag::Strikethrough => Mark::Strike,
            Tag::Link {
                link_type,
                dest_url,
                ..
            } => Mark::Link(href(link_type, dest_url)),
            Tag::Image { dest_url, .. } => {
                self.image = Some(Image {
                    source: dest_url.into_string(),
                    alt: String::new(),
                    nested: 0,
                });
                return Ok(());
            }
            block => return self.open(block),
        };
        self.marks.push(mark);
        Ok(())
    }

    fn open(&mut self, tag: Tag) -> Result<()> {
        let block = match tag {
            Tag::Paragraph => Block::Paragraph,
            Tag::Heading { level, .. } => Block::Heading(level as u8),
            Tag::BlockQuote(_) => Block::Quote,
            Tag::CodeBlock(kind) => Block::Code {
                language: match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().map(str::to_string)
                    }
                    CodeBlockKind::Indented => None,
                },
                text: String::new(),
            },
            Tag::HtmlBlock => Block::Html(String::new()),
            Tag::List(start) => Block::List(start),
            Tag::Item => Block::Item,
            Tag::Table(_) => Block::Table,
            Tag::TableHead => Block::Row { header: true },
            Tag::TableRow => Block::Row { header: false },
            Tag::TableCell => Block::Cell,
            other => unreachable!("parse_markdown enables no extension that opens {other:?}"),
        };
        self.close_implicit()?;
        if self.frames.len() >= MAX_DEPTH {
            bail!(
                "Markdown nests blocks more than {MAX_DEPTH} levels deep, which ADF output does not support"
            );
        }
        self.frames.push(Frame {
            block,
            content: Vec::new(),
        });
        Ok(())
    }

    fn end(&mut self, end: TagEnd) -> Result<()> {
        if matches!(
            end,
            TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough | TagEnd::Link
        ) {
            self.marks.pop();
            return Ok(());
        }
        self.close_implicit()?;
        let frame = self
            .frames
            .pop()
            .expect("an end event closes an open block");
        let header = matches!(
            self.frames.last(),
            Some(Frame {
                block: Block::Row { header: true },
                ..
            })
        );
        self.append(node(frame, header))
    }

    fn close_implicit(&mut self) -> Result<()> {
        if let Some(Frame {
            block: Block::ImplicitParagraph,
            ..
        }) = self.frames.last()
        {
            let frame = self.frames.pop().expect("checked above");
            self.append(node(frame, false))?;
        }
        Ok(())
    }

    fn append(&mut self, node: Value) -> Result<()> {
        let Some(parent) = self.frames.last_mut() else {
            self.doc.push(node);
            return Ok(());
        };
        let kind = node["type"].as_str().unwrap_or_default();
        if parent.block.holds_only_simple_blocks()
            && !matches!(
                kind,
                "paragraph" | "bulletList" | "orderedList" | "codeBlock"
            )
        {
            let container = match parent.block {
                Block::Item => "a list item",
                _ => "a quote",
            };
            let child = match kind {
                "heading" => "a heading",
                "blockquote" => "a quote",
                "table" => "a table",
                _ => "a horizontal rule",
            };
            bail!(
                "Markdown places {child} inside {container}, which ADF cannot represent; \
                 move it outside, or pass the field as ADF"
            );
        }
        parent.content.push(node);
        Ok(())
    }

    fn inline_frame(&mut self) -> &mut Frame {
        if !self.frames.last().is_some_and(|f| f.block.holds_inline()) {
            self.frames.push(Frame {
                block: Block::ImplicitParagraph,
                content: Vec::new(),
            });
        }
        self.frames.last_mut().expect("an inline frame is open")
    }

    fn text(&mut self, text: &str, marks: Vec<Value>) {
        if text.is_empty() {
            return;
        }
        let content = &mut self.inline_frame().content;
        if let Some(last) = content.last_mut()
            && last["type"] == "text"
            && last
                .get("marks")
                .and_then(Value::as_array)
                .map_or(&[][..], Vec::as_slice)
                == marks.as_slice()
            && let Some(Value::String(existing)) = last.get_mut("text")
        {
            existing.push_str(text);
            return;
        }
        let mut node = json!({ "type": "text", "text": text });
        if !marks.is_empty() {
            node["marks"] = Value::Array(marks);
        }
        content.push(node);
    }

    /// The active marks, each kind once: ADF rejects a node carrying the same
    /// mark twice, which nested `**a **b** c**` would otherwise produce.
    fn mark_values(&self) -> Vec<Value> {
        let mut kinds = Vec::new();
        let mut values = Vec::new();
        for mark in &self.marks {
            if !kinds.contains(&mark.kind()) {
                kinds.push(mark.kind());
                values.push(mark.to_value());
            }
        }
        values
    }

    fn finish(mut self) -> Result<Value> {
        self.close_implicit()?;
        debug_assert!(self.frames.is_empty(), "every block was closed");
        let content = if self.doc.is_empty() {
            vec![empty_paragraph()]
        } else {
            self.doc
        };
        Ok(json!({ "type": "doc", "version": 1, "content": content }))
    }
}

fn node(frame: Frame, in_header_row: bool) -> Value {
    let Frame { block, content } = frame;
    let or_empty_paragraph = |content: Vec<Value>| {
        if content.is_empty() {
            vec![empty_paragraph()]
        } else {
            content
        }
    };
    match block {
        Block::Paragraph | Block::ImplicitParagraph => {
            json!({ "type": "paragraph", "content": content })
        }
        Block::Heading(level) => {
            json!({ "type": "heading", "attrs": { "level": level }, "content": content })
        }
        Block::Quote => json!({ "type": "blockquote", "content": or_empty_paragraph(content) }),
        Block::List(None) => json!({ "type": "bulletList", "content": content }),
        Block::List(Some(start)) => {
            let mut list = json!({ "type": "orderedList", "content": content });
            if start != 1 {
                list["attrs"] = json!({ "order": start });
            }
            list
        }
        Block::Item => json!({ "type": "listItem", "content": or_empty_paragraph(content) }),
        Block::Code { language, text } => {
            let text = text.trim_end_matches('\n');
            let mut code = json!({ "type": "codeBlock", "content": [] });
            if !text.is_empty() {
                code["content"] = json!([{ "type": "text", "text": text }]);
            }
            if let Some(language) = language {
                code["attrs"] = json!({ "language": language });
            }
            code
        }
        Block::Html(html) => {
            let mut content = Vec::new();
            for line in html.lines().filter(|line| !line.is_empty()) {
                if !content.is_empty() {
                    content.push(json!({ "type": "hardBreak" }));
                }
                content.push(json!({ "type": "text", "text": line }));
            }
            json!({ "type": "paragraph", "content": content })
        }
        Block::Table => json!({ "type": "table", "content": content }),
        Block::Row { .. } => json!({ "type": "tableRow", "content": content }),
        Block::Cell => json!({
            "type": if in_header_row { "tableHeader" } else { "tableCell" },
            "content": [{ "type": "paragraph", "content": content }],
        }),
    }
}

fn empty_paragraph() -> Value {
    json!({ "type": "paragraph", "content": [] })
}

fn link_mark(href: &str) -> Value {
    json!({ "type": "link", "attrs": { "href": href } })
}

fn href(link_type: LinkType, destination: CowStr) -> String {
    match link_type {
        LinkType::Email => format!("mailto:{destination}"),
        _ => destination.into_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(text: &str) -> Value {
        json!({ "type": "text", "text": text })
    }

    fn marked(text: &str, marks: Value) -> Value {
        json!({ "type": "text", "text": text, "marks": marks })
    }

    fn paragraph(content: Value) -> Value {
        json!({ "type": "paragraph", "content": content })
    }

    fn doc(content: Value) -> Value {
        json!({ "type": "doc", "version": 1, "content": content })
    }

    #[test]
    fn inline_formatting_maps_to_marks_and_breaks() {
        let adf = markdown_to_adf(
            "# Release *notes*\n\n\
             A **bold *nested*** ~~gone~~ [site](https://example.com)\n\
             joined line  \nhard break `x` [`code link`](https://c.example) **`bare`**",
        )
        .unwrap();

        assert_eq!(
            adf,
            doc(json!([
                {
                    "type": "heading",
                    "attrs": { "level": 1 },
                    "content": [text("Release "), marked("notes", json!([{ "type": "em" }]))]
                },
                paragraph(json!([
                    text("A "),
                    marked("bold ", json!([{ "type": "strong" }])),
                    marked("nested", json!([{ "type": "strong" }, { "type": "em" }])),
                    text(" "),
                    marked("gone", json!([{ "type": "strike" }])),
                    text(" "),
                    marked("site", json!([link_mark("https://example.com")])),
                    text(" joined line"),
                    { "type": "hardBreak" },
                    text("hard break "),
                    marked("x", json!([{ "type": "code" }])),
                    text(" "),
                    marked(
                        "code link",
                        json!([{ "type": "code" }, link_mark("https://c.example")])
                    ),
                    text(" "),
                    marked("bare", json!([{ "type": "code" }])),
                ])),
            ]))
        );
    }

    #[test]
    fn lists_quotes_code_and_rules_nest_as_adf_requires() {
        let adf = markdown_to_adf(
            "- tight\n  - nested\n\n3. third\n4. fourth\n\n> quoted\n\n\
             ```rust title\nfn main() {}\n```\n\n---\n\n    indented\n",
        )
        .unwrap();

        assert_eq!(
            adf,
            doc(json!([
                {
                    "type": "bulletList",
                    "content": [{
                        "type": "listItem",
                        "content": [
                            paragraph(json!([text("tight")])),
                            {
                                "type": "bulletList",
                                "content": [{
                                    "type": "listItem",
                                    "content": [paragraph(json!([text("nested")]))]
                                }]
                            }
                        ]
                    }]
                },
                {
                    "type": "orderedList",
                    "attrs": { "order": 3 },
                    "content": [
                        { "type": "listItem", "content": [paragraph(json!([text("third")]))] },
                        { "type": "listItem", "content": [paragraph(json!([text("fourth")]))] }
                    ]
                },
                { "type": "blockquote", "content": [paragraph(json!([text("quoted")]))] },
                {
                    "type": "codeBlock",
                    "attrs": { "language": "rust" },
                    "content": [text("fn main() {}")]
                },
                { "type": "rule" },
                { "type": "codeBlock", "content": [text("indented")] },
            ]))
        );
    }

    #[test]
    fn tables_images_and_raw_html_become_their_adf_counterparts() {
        let adf = markdown_to_adf(
            "| Key | Owner |\n|:---|---|\n| A-1 | |\n\n\
             ![chart](https://img.example/c.png) [![badge](https://img.example/b.svg)](https://ci.example) \
             ![a ![b](c) d](e)\n\n\
             <div>raw</div>\n\nkeep <b>tags</b>\n",
        )
        .unwrap();

        let cell =
            |kind: &str, content: Value| json!({ "type": kind, "content": [paragraph(content)] });
        assert_eq!(
            adf,
            doc(json!([
                {
                    "type": "table",
                    "content": [
                        {
                            "type": "tableRow",
                            "content": [
                                cell("tableHeader", json!([text("Key")])),
                                cell("tableHeader", json!([text("Owner")]))
                            ]
                        },
                        {
                            "type": "tableRow",
                            "content": [cell("tableCell", json!([text("A-1")])), cell("tableCell", json!([]))]
                        }
                    ]
                },
                paragraph(json!([
                    marked("chart", json!([link_mark("https://img.example/c.png")])),
                    text(" "),
                    marked("badge", json!([link_mark("https://ci.example")])),
                    text(" "),
                    marked("a b d", json!([link_mark("e")])),
                ])),
                paragraph(json!([text("<div>raw</div>")])),
                paragraph(json!([text("keep <b>tags</b>")])),
            ]))
        );
    }

    #[test]
    fn empty_containers_hold_the_empty_paragraph_adf_requires() {
        assert_eq!(
            markdown_to_adf("").unwrap(),
            doc(json!([paragraph(json!([]))]))
        );
        assert_eq!(
            markdown_to_adf("-\n\n>\n").unwrap(),
            doc(json!([
                {
                    "type": "bulletList",
                    "content": [{ "type": "listItem", "content": [paragraph(json!([]))] }]
                },
                { "type": "blockquote", "content": [paragraph(json!([]))] },
            ]))
        );
    }

    #[test]
    fn nesting_adf_cannot_hold_is_refused_with_its_location() {
        for (markdown, expected) in [
            ("- # Title", "a heading inside a list item"),
            ("> > nested", "a quote inside a quote"),
            (
                "- item\n\n  | a |\n  |---|\n  | b |",
                "a table inside a list item",
            ),
            ("> ---\n> more", "a horizontal rule inside a quote"),
        ] {
            let err = markdown_to_adf(markdown).unwrap_err().to_string();
            assert!(err.contains(expected), "{markdown:?}: {err}");
        }
        let nested = format!("{}deep", "- ".repeat(MAX_DEPTH));
        let err = markdown_to_adf(&nested).unwrap_err().to_string();
        assert!(err.contains("levels deep"), "{err}");
    }
}
