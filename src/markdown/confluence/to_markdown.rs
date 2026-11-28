use std::cell::RefCell;
use std::rc::Rc;
use std::sync::LazyLock;

use htmd::element_handler::{Context, HandlerResult, Handlers};
use htmd::node::Node;
use htmd::options::{BulletListMarker, Options};
use htmd::{Element, HtmlToMarkdown};
use markup5ever_rcdom::{self as rcdom, Handle, NodeData, RcDom};
use xml5ever::driver::{XmlParseOpts, parse_document};
use xml5ever::tendril::TendrilSink;
use xml5ever::{Attribute, LocalName, QualName, ns};

use crate::markdown::common::fenced_code_block;

/// Convert Confluence storage format to Markdown.
///
/// Storage format is XML, so it is parsed as XML: CDATA keeps code verbatim,
/// `ac:`/`ri:` elements nest as written, and HTML entities resolve. Confluence
/// elements are rendered by the handlers below and standard XHTML by `htmd`.
/// The result is a reading view; constructs Markdown cannot express become
/// readable placeholders, and `--format storage` is the lossless form.
pub fn confluence_to_markdown(storage: &str) -> String {
    let document = format!(
        r#"<storage xmlns:ac="http://atlassian.com/content" xmlns:ri="http://atlassian.com/resource/identifier">{storage}</storage>"#
    );
    let dom = parse_document(RcDom::default(), XmlParseOpts::default()).one(document.as_str());
    match qualified(&dom.document) {
        // Only string input is parsed, and parsing is the only step that can
        // fail; a tree converts without I/O.
        Some(tree) => CONVERTER
            .convert(&tree)
            .expect("htmd renders a parsed tree without I/O"),
        None => String::new(),
    }
}

/// Copy the parsed tree with every prefixed name spelled out (`ac:link`), as
/// storage format writes it. `htmd` matches elements by local name alone, and
/// `link` is an HTML block element, so the bare local name would reflow the
/// text around a Confluence link. Comments and processing instructions carry
/// no content and are left out.
fn qualified(node: &Handle) -> Option<Handle> {
    let data = match &node.data {
        NodeData::Document => NodeData::Document,
        NodeData::Text { contents } => NodeData::Text {
            contents: RefCell::new(contents.borrow().clone()),
        },
        NodeData::Element { name, attrs, .. } => NodeData::Element {
            name: qualified_name(name),
            attrs: RefCell::new(
                attrs
                    .borrow()
                    .iter()
                    .map(|a| Attribute {
                        name: qualified_name(&a.name),
                        value: a.value.clone(),
                    })
                    .collect(),
            ),
            template_contents: RefCell::new(None),
            mathml_annotation_xml_integration_point: false,
        },
        NodeData::Doctype { .. }
        | NodeData::Comment { .. }
        | NodeData::ProcessingInstruction { .. } => return None,
    };
    let copy = rcdom::Node::new(data);
    for child in node.children.borrow().iter().filter_map(qualified) {
        child.parent.set(Some(Rc::downgrade(&copy)));
        copy.children.borrow_mut().push(child);
    }
    Some(copy)
}

fn qualified_name(name: &QualName) -> QualName {
    match &name.prefix {
        Some(prefix) => QualName::new(
            None,
            ns!(),
            LocalName::from(format!("{prefix}:{}", name.local)),
        ),
        None => QualName::new(None, ns!(), name.local.clone()),
    }
}

static CONVERTER: LazyLock<HtmlToMarkdown> = LazyLock::new(|| {
    HtmlToMarkdown::builder()
        .options(Options {
            bullet_list_marker: BulletListMarker::Dash,
            ul_bullet_spacing: 1,
            ol_number_spacing: 1,
            ..Options::default()
        })
        .add_handler(vec!["pre"], preformatted)
        .add_handler(vec!["time"], time)
        .add_handler(vec!["ac:structured-macro"], structured_macro)
        .add_handler(vec!["ac:task-list"], task_list)
        .add_handler(vec!["ac:image"], image)
        .add_handler(vec!["ac:link"], link)
        .add_handler(vec!["ac:emoticon"], emoticon)
        .add_handler(vec!["ac:adf-extension"], adf_extension)
        .skip_tags(vec![
            "ac:parameter",
            "ac:placeholder",
            "ac:task-id",
            "ac:task-uuid",
            "ac:task-status",
            "ac:adf-attribute",
            "ac:adf-parameter",
        ])
        .build()
});

fn preformatted(_: &dyn Handlers, element: &Element) -> Option<HandlerResult> {
    Some(block(fenced_code_block("", &text(element.node))).into())
}

fn time(handlers: &dyn Handlers, element: &Element) -> Option<HandlerResult> {
    match attribute(element.node, "datetime") {
        Some(datetime) => Some(datetime.into()),
        None => handlers.fallback(element),
    }
}

fn structured_macro(handlers: &dyn Handlers, element: &Element) -> Option<HandlerResult> {
    let node = element.node;
    let name = attribute(node, "ac:name").unwrap_or_default();
    let parameter = |key: &str| macro_parameter(node, key);
    let rich_body = || {
        child(node, "ac:rich-text-body")
            .map(|body| trim_blank_lines(&handlers.walk_children_content(&body, Context::BLOCK)))
            .unwrap_or_default()
    };
    let plain_body = || {
        child(node, "ac:plain-text-body")
            .map(|body| text(&body))
            .unwrap_or_default()
    };

    let rendered = match name.as_str() {
        "code" | "noformat" => {
            let language = parameter("language").unwrap_or_default();
            let code = fenced_code_block(&language, &plain_body());
            match parameter("title") {
                Some(title) => block(format!("**{}**\n\n{code}", escape_inline(&title))),
                None => block(code),
            }
        }
        "info" | "note" | "warning" | "tip" | "error" | "panel" => {
            let kind = (name != "panel").then(|| name.to_uppercase());
            let label = match (kind, parameter("title")) {
                (Some(kind), Some(title)) => Some(format!("{kind} - {title}")),
                (Some(kind), None) => Some(kind),
                (None, title) => title,
            };
            block(quote(label.as_deref(), &rich_body()))
        }
        "expand" => {
            let title = parameter("title").unwrap_or_else(|| "Details".into());
            block(format!("**{}**\n\n{}", escape_inline(&title), rich_body()))
        }
        "plantuml" => block(fenced_code_block("plantuml", &plain_body())),
        "toc" | "anchor" => String::new(),
        "jira" => {
            let reference = parameter("key").or_else(|| parameter("jqlQuery"));
            placeholder("JIRA", reference.as_deref())
        }
        "status" => status(
            parameter("title"),
            parameter("colour").or(parameter("color")),
        ),
        "drawio" => block(placeholder(
            "Draw.io",
            parameter("diagramDisplayName")
                .or_else(|| parameter("diagramName"))
                .as_deref(),
        )),
        "gliffy" => block(placeholder(
            "Gliffy",
            parameter("displayName")
                .or_else(|| parameter("name"))
                .as_deref(),
        )),
        "lucidchart" => block(match parameter("documentId") {
            Some(id) => format!("[Lucidchart](https://lucid.app/documents/view/{id})"),
            None => placeholder("Lucidchart", None),
        }),
        "miro" => block(match parameter("boardId") {
            Some(id) => format!("[Miro](https://miro.com/app/board/{id})"),
            None => placeholder("Miro", None),
        }),
        "children" => block(placeholder("Child Pages", None)),
        "pagetree" => block(placeholder("Page Tree", None)),
        "recently-updated" => block(placeholder("Recently Updated", None)),
        "widget" | "iframe" | "html" => block(placeholder(
            "Embed",
            parameter("url").or_else(|| parameter("src")).as_deref(),
        )),
        _ => match child(node, "ac:rich-text-body") {
            Some(_) => block(rich_body()),
            None => placeholder(&format!("Macro: {name}"), None),
        },
    };
    Some(rendered.into())
}

fn macro_parameter(node: &Node, key: &str) -> Option<String> {
    node.children()
        .iter()
        .find(|c| is(c, "ac:parameter") && attribute(c, "ac:name").as_deref() == Some(key))
        .map(|c| text(&c).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn status(title: Option<String>, colour: Option<String>) -> String {
    let indicator = match colour.as_deref().map(str::to_lowercase).as_deref() {
        Some("green") => "[OK]",
        Some("yellow") => "[WARN]",
        Some("red") => "[ERR]",
        Some("blue") => "[INFO]",
        _ => "[STATUS]",
    };
    let title = title.unwrap_or_else(|| "STATUS".into()).to_uppercase();
    format!("{indicator} {}", escape_inline(&title))
}

fn task_list(handlers: &dyn Handlers, element: &Element) -> Option<HandlerResult> {
    Some(block(tasks(handlers, element.node)).into())
}

fn tasks(handlers: &dyn Handlers, list: &Node) -> String {
    let mut items: Vec<String> = Vec::new();
    for entry in list.children().iter() {
        if is(&entry, "ac:task") {
            let done =
                child(&entry, "ac:task-status").is_some_and(|s| text(&s).trim() == "complete");
            let body = child(&entry, "ac:task-body")
                .map(|b| trim_blank_lines(&handlers.walk_children_content(&b, Context::BLOCK)))
                .unwrap_or_default();
            let marker = if done { "- [x] " } else { "- [ ] " };
            items.push(format!("{marker}{}", indent_continuation(&body, "  ")));
        } else if is(&entry, "ac:task-list") {
            let nested = indent_all(&tasks(handlers, &entry), "  ");
            match items.last_mut() {
                Some(previous) => {
                    previous.push('\n');
                    previous.push_str(&nested);
                }
                None => items.push(nested),
            }
        }
    }
    items.join("\n")
}

fn image(_: &dyn Handlers, element: &Element) -> Option<HandlerResult> {
    let node = element.node;
    let alt = attribute(node, "ac:alt")
        .or_else(|| attribute(node, "ac:title"))
        .unwrap_or_default();
    let source = child(node, "ri:attachment")
        .and_then(|a| attribute(&a, "ri:filename"))
        .or_else(|| child(node, "ri:url").and_then(|u| attribute(&u, "ri:value")));
    let rendered = match source {
        Some(source) => format!("![{}]({})", escape_inline(&alt), destination(&source)),
        None => placeholder("Image", Some(alt.as_str()).filter(|a| !a.is_empty())),
    };
    Some(rendered.into())
}

fn link(handlers: &dyn Handlers, element: &Element) -> Option<HandlerResult> {
    let node = element.node;
    let label = child(node, "ac:link-body")
        .map(|body| {
            handlers
                .walk_children_content(&body, Context::INLINE)
                .trim()
                .to_string()
        })
        .or_else(|| child(node, "ac:plain-text-link-body").map(|b| escape_inline(text(&b).trim())))
        .filter(|label| !label.is_empty());
    let anchor = attribute(node, "ac:anchor").map(|a| format!("#{a}"));

    let (target, default_label) = if let Some(page) = child(node, "ri:page") {
        let title = attribute(&page, "ri:content-title").unwrap_or_default();
        let path = match attribute(&page, "ri:space-key") {
            Some(space) => format!("page:{space}/{title}"),
            None => format!("page:{title}"),
        };
        (Some(path + anchor.as_deref().unwrap_or("")), Some(title))
    } else if let Some(user) = child(node, "ri:user") {
        let id = attribute(&user, "ri:account-id").or_else(|| attribute(&user, "ri:userkey"));
        match (&label, id) {
            (Some(_), Some(id)) => (Some(format!("@{id}")), None),
            (None, Some(id)) => return Some(format!("@{id}").into()),
            (_, None) => (None, None),
        }
    } else if let Some(attachment) = child(node, "ri:attachment") {
        let filename = attribute(&attachment, "ri:filename");
        (
            filename.as_ref().map(|f| format!("attachment:{f}")),
            filename,
        )
    } else if let Some(url) = child(node, "ri:url") {
        (attribute(&url, "ri:value"), None)
    } else {
        (anchor, None)
    };

    let label = label.or_else(|| default_label.map(|l| escape_inline(&l)));
    let rendered = match (label, target) {
        (Some(label), Some(target)) => format!("[{label}]({})", destination(&target)),
        (Some(label), None) => label,
        (None, Some(target)) => format!("[{}]({})", escape_inline(&target), destination(&target)),
        (None, None) => String::new(),
    };
    Some(rendered.into())
}

fn emoticon(_: &dyn Handlers, element: &Element) -> Option<HandlerResult> {
    let node = element.node;
    if let Some(emoji) = attribute(node, "ac:emoji-fallback") {
        return Some(emoji.into());
    }
    let name = attribute(node, "ac:name").unwrap_or_default();
    let shortcut = match name.as_str() {
        "smile" | "smiley" => ":)",
        "sad" => ":(",
        "wink" => ";)",
        "laugh" => ":D",
        "thumbs-up" => "(y)",
        "thumbs-down" => "(n)",
        "tick" | "check" => "[x]",
        "cross" | "error" | "warning" => "[!]",
        "information" | "info" => "(i)",
        "question" => "(?)",
        "light-on" | "idea" => "(!)",
        "star" => "(*)",
        "heart" => "<3",
        other => other,
    };
    Some(shortcut.to_string().into())
}

fn adf_extension(handlers: &dyn Handlers, element: &Element) -> Option<HandlerResult> {
    let node = element.node;
    let adf_node = child(node, "ac:adf-node");
    let kind = adf_node
        .as_ref()
        .and_then(|n| attribute(n, "type"))
        .unwrap_or_default();
    let content = adf_node
        .as_ref()
        .and_then(|n| child(n, "ac:adf-content"))
        .map(|c| trim_blank_lines(&handlers.walk_children_content(&c, Context::BLOCK)));

    let rendered = if kind == "panel" {
        let label = adf_node
            .as_ref()
            .and_then(|n| adf_value(n, "ac:adf-attribute", "panel-type"))
            .map(|panel| panel.to_uppercase());
        quote(label.as_deref(), &content.unwrap_or_default())
    } else if let Some(content) = content.filter(|c| !c.is_empty()) {
        content
    } else if let Some(name) = adf_node.as_ref().and_then(|n| {
        ["diagram-display-name", "diagramDisplayName", "diagram-name"]
            .iter()
            .find_map(|key| adf_value(n, "ac:adf-parameter", key))
    }) {
        placeholder("Draw.io", Some(&name))
    } else {
        child(node, "ac:adf-fallback")
            .map(|f| trim_blank_lines(&handlers.walk_children_content(&f, Context::BLOCK)))
            .unwrap_or_default()
    };
    Some(block(rendered).into())
}

/// The text of the first descendant `element` whose `key` is `key`.
fn adf_value(node: &Node, element: &str, key: &str) -> Option<String> {
    node.children()
        .iter()
        .find_map(|c| c.find(|d| is(d, element) && attribute(d, "key").as_deref() == Some(key)))
        .map(|d| text(&d).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn quote(label: Option<&str>, body: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    if let Some(label) = label {
        lines.push(format!("> **{}**", escape_inline(label)));
        if !body.is_empty() {
            lines.push(">".into());
        }
    }
    lines.extend(body.lines().map(|line| {
        if line.is_empty() {
            ">".to_string()
        } else {
            format!("> {line}")
        }
    }));
    lines.join("\n")
}

fn placeholder(kind: &str, detail: Option<&str>) -> String {
    match detail {
        Some(detail) => format!("[{kind}: {}]", escape_inline(detail)),
        None => format!("[{kind}]"),
    }
}

/// A block-level rendering, separated from its neighbours the way `htmd`
/// separates paragraphs. An empty rendering stays empty.
fn block(content: String) -> String {
    if content.is_empty() {
        content
    } else {
        format!("\n\n{content}\n\n")
    }
}

fn trim_blank_lines(content: &str) -> String {
    content.trim_matches('\n').to_string()
}

fn indent_continuation(content: &str, indent: &str) -> String {
    content.replace('\n', &format!("\n{indent}"))
}

fn indent_all(content: &str, indent: &str) -> String {
    content
        .lines()
        .map(|line| format!("{indent}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Text placed inside Markdown link text, image alt text or emphasis.
fn escape_inline(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '[' | ']' | '*' | '_' | '`') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// A link destination; one holding spaces, parentheses or angle brackets is
/// written in the angle-bracket form CommonMark provides for it.
fn destination(target: &str) -> String {
    if target
        .chars()
        .any(|c| c.is_whitespace() || "()<>".contains(c))
    {
        format!("<{}>", target.replace('<', "\\<").replace('>', "\\>"))
    } else {
        target.to_string()
    }
}

fn is(node: &Node, name: &str) -> bool {
    node.tag_name() == Some(name)
}

fn attribute(node: &Node, name: &str) -> Option<String> {
    node.attr(name).map(|value| value.to_string())
}

/// The first child element named `name`. Siblings are walked rather than
/// `children()` iterated because only the walk yields nodes that outlive the
/// borrow of their parent.
fn child<'a>(node: &Node<'a>, name: &str) -> Option<Node<'a>> {
    std::iter::successors(node.first_child(), Node::next_sibling).find(|c| is(c, name))
}

fn text(node: &Node) -> String {
    let mut collected = String::new();
    collect_text(node, &mut collected);
    collected
}

fn collect_text(node: &Node, into: &mut String) {
    if let Some(text) = node.text() {
        into.push_str(&text);
    }
    for child in node.children().iter() {
        collect_text(&child, into);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::markdown_to_confluence;

    #[test]
    fn renders_each_storage_construct() {
        let cases = [
            (
                "code keeps its lines, indentation and markup verbatim",
                r#"<ac:structured-macro ac:name="code" ac:macro-id="m"><ac:parameter ac:name="language">python</ac:parameter><ac:plain-text-body><![CDATA[    def f():
        return "<b>x</b>"


    g = 1]]></ac:plain-text-body></ac:structured-macro>"#,
                "```python\n    def f():\n        return \"<b>x</b>\"\n\n\n    g = 1\n```",
            ),
            (
                "a fence outlasts the backticks inside the code",
                r#"<ac:structured-macro ac:name="code"><ac:plain-text-body><![CDATA[a ``` b]]></ac:plain-text-body></ac:structured-macro>"#,
                "````\na ``` b\n````",
            ),
            (
                "preformatted text is a code block",
                "<pre>a\n  b</pre>",
                "```\na\n  b\n```",
            ),
            (
                "tasks keep one line each and nest",
                r#"<ac:task-list><ac:task><ac:task-id>1</ac:task-id><ac:task-status>incomplete</ac:task-status><ac:task-body>a <strong>b</strong></ac:task-body></ac:task><ac:task-list><ac:task><ac:task-status>complete</ac:task-status><ac:task-body>c</ac:task-body></ac:task></ac:task-list></ac:task-list>"#,
                "- [ ] a **b**\n  - [x] c",
            ),
            (
                "a panel quotes its whole body under its label",
                r#"<ac:structured-macro ac:name="info"><ac:parameter ac:name="title">T</ac:parameter><ac:rich-text-body><p>one</p><p>two</p></ac:rich-text-body></ac:structured-macro><p>after</p>"#,
                "> **INFO - T**\n>\n> one\n>\n> two\n\nafter",
            ),
            (
                "a macro nested in another renders in place",
                r#"<ac:structured-macro ac:name="expand"><ac:rich-text-body><ac:structured-macro ac:name="code"><ac:plain-text-body><![CDATA[x]]></ac:plain-text-body></ac:structured-macro></ac:rich-text-body></ac:structured-macro><p>end</p>"#,
                "**Details**\n\n```\nx\n```\n\nend",
            ),
            (
                "images use the syntax the writer accepts",
                r#"<p><ac:image ac:alt="d"><ri:attachment ri:filename="a b.png"/></ac:image> <ac:image><ri:url ri:value="https://x.io/i.png"/></ac:image></p>"#,
                "![d](<a b.png>) ![](https://x.io/i.png)",
            ),
            (
                "links keep their targets and the text around them",
                r#"<p><ac:link><ri:page ri:space-key="ENG" ri:content-title="Run Book"/><ac:plain-text-link-body><![CDATA[runbook]]></ac:plain-text-link-body></ac:link>, <ac:link><ri:user ri:account-id="557058:a"/></ac:link>, <ac:link ac:anchor="s"><ac:link-body>jump</ac:link-body></ac:link></p>"#,
                "[runbook](<page:ENG/Run Book>), @557058:a, [jump](#s)",
            ),
            (
                "inline macros, dates and emoji stay in their sentence",
                r#"<p>Is <ac:structured-macro ac:name="status"><ac:parameter ac:name="title">Done</ac:parameter><ac:parameter ac:name="colour">Green</ac:parameter></ac:structured-macro> per <ac:structured-macro ac:name="jira"><ac:parameter ac:name="key">ENG-1</ac:parameter></ac:structured-macro> on <time datetime="2026-11-16" /> <ac:emoticon ac:name="tick" ac:emoji-fallback="✅"/></p>"#,
                "Is [OK] DONE per [JIRA: ENG-1] on 2026-11-16 ✅",
            ),
            (
                "an ADF panel is quoted under its type",
                r#"<ac:adf-extension><ac:adf-node type="panel"><ac:adf-attribute key="panel-type">note</ac:adf-attribute><ac:adf-content><p>Body</p></ac:adf-content></ac:adf-node><ac:adf-fallback><div>Body</div></ac:adf-fallback></ac:adf-extension>"#,
                "> **NOTE**\n>\n> Body",
            ),
            (
                "macro bookkeeping and placeholders are not content",
                r#"<ac:structured-macro ac:name="toc"/><p>a<ac:placeholder>Type here</ac:placeholder></p><ac:structured-macro ac:name="unknown"><ac:parameter ac:name="x">secret-looking-value</ac:parameter></ac:structured-macro>"#,
                "a\n\n[Macro: unknown]",
            ),
            (
                "entities resolve and a literal ampersand is escaped",
                "Tom&nbsp;&amp; Jerry",
                "Tom\u{a0}\\& Jerry",
            ),
        ];
        for (case, storage, expected) in cases {
            assert_eq!(confluence_to_markdown(storage), expected, "{case}");
        }
    }

    #[test]
    fn markdown_the_writer_produced_reads_back_unchanged() {
        let markdown = "# Title\n\n```python\ndef f():\n    return 1\n\n\nx = 2\n```\n\n- item\n\nTom \\& Jerry pay \\$5\n\n![d](diagram.png)";
        let storage = markdown_to_confluence(markdown).unwrap();
        assert_eq!(confluence_to_markdown(&storage), markdown);
    }

    #[test]
    fn markup_outside_the_content_model_is_dropped_without_panicking() {
        let storage = "<?pi data?><!-- note --><p>kept</p>";
        assert_eq!(confluence_to_markdown(storage), "kept");
    }
}
