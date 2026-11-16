# markdown module

## Reading: two converters, different pipelines

- `adf::adf_to_markdown` — walks Atlassian Document Format JSON (Jira). Handles `paragraph`, `heading`, `bulletList`, `orderedList`, `codeBlock`, `panel`, `table`, `mention`, `inlineCard`, marks, etc. Unknown blocks preserve convertible child blocks with an `<!-- Unsupported: NAME -->` marker; those without convertible children are omitted. Unknown inline nodes are omitted.
- `confluence::confluence_to_markdown` — parses storage format as XML with `xml5ever`, spells prefixed names out (`ac:link`) and renders the tree through `htmd`, whose handlers cover the Confluence elements. CDATA, nesting and entities are the parser's job: storage markup is never matched as text. `htmd` compares local names only, which is why prefixes are kept in the name.

In the ADF reader, block recursion depth is independent of list indentation: every recursive block conversion passes through `adf::blocks::convert_block_node`, and exceeding the depth limit emits a truncation marker and preserves subsequent sibling blocks. The Confluence reader leaves nesting to `htmd` and has no depth marker.

## Writing: one parser, two renderers

`common::parse_markdown` fixes the dialect writes accept (CommonMark, GFM tables and strikethrough) for both products.

- `adf::markdown_to_adf` builds the ADF tree from parser events. ADF's content rules are enforced where a child is appended: a list item or quote takes only paragraphs, lists and code blocks, and anything else is an error naming the construct. Do not reshape such content into something ADF accepts; the caller decides. Nesting is bounded by the same `MAX_DEPTH` the reader truncates at.
- `confluence::markdown_to_confluence` renders HTML with pulldown-cmark and substitutes the code macro, the documented strikethrough span and `ac:image`. Its output must stay well-formed XML: characters XML 1.0 excludes are refused before parsing, and raw HTML is emitted as text because HTML need not be XML.

Raw HTML is literal text in both renderers. Markdown tag names cannot carry the `ac:` namespace, so storage-format macros are written with `--format storage` rather than embedded.

Task-list markers and other extensions stay disabled until their ADF and storage shapes are verified against the products.

## Do not add formatting features here unless they are in the Atlassian schema

New ADF node types should match Atlassian's published ADF spec. Don't invent node names. If an unfamiliar node type appears in real data, check the ADF docs first and handle it in `adf/blocks.rs` or `adf/inline.rs`.

## Reading is a view, storage is the lossless form

Markdown cannot express macros, mentions, layouts or most ADF nodes; the readers render them as readable placeholders. Code blocks, lists, quotes, tables, links and hard breaks read back as the Markdown that wrote them, and a Confluence image does too; the Jira writer stores an image as a link. Block content is joined structurally — list items indent to their marker's width and code keeps its blank lines — never by normalizing whitespace across the document. Don't add round-trip support for constructs Markdown lacks; `--format storage` (or `adf`) is the form for editing them.
