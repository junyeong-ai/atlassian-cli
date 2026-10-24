# markdown module

## Reading: two converters, different pipelines

- `adf::adf_to_markdown` — walks Atlassian Document Format JSON (Jira). Handles `paragraph`, `heading`, `bulletList`, `orderedList`, `codeBlock`, `panel`, `table`, `mention`, `inlineCard`, marks, etc. Unknown blocks preserve convertible child blocks with an `<!-- Unsupported: NAME -->` marker; those without convertible children are omitted. Unknown inline nodes are omitted.
- `confluence::confluence_to_markdown` — runs `htmd` on HTML storage format. Confluence HTML includes `ac:*` / `ri:*` namespaced tags and macro blocks; `confluence::cleanup` strips residue (schema-version, mxgraph base64, etc.) before conversion.

Block recursion depth is independent of list indentation. Every recursive block conversion passes through `convert_block_node`; exceeding the depth limit emits a truncation marker and preserves subsequent sibling blocks.

## Writing: one parser, two renderers

`common::parse_markdown` fixes the dialect writes accept (CommonMark, GFM tables and strikethrough) for both products.

- `adf::markdown_to_adf` builds the ADF tree from parser events. ADF's content rules are enforced where a child is appended: a list item or quote takes only paragraphs, lists and code blocks, and anything else is an error naming the construct. Do not reshape such content into something ADF accepts; the caller decides. Nesting is bounded by the same `MAX_DEPTH` the reader truncates at.
- `confluence::markdown_to_confluence` renders HTML with pulldown-cmark and substitutes the code macro, the documented strikethrough span and `ac:image`. Its output must stay well-formed XML: characters XML 1.0 excludes are refused before parsing, and raw HTML is emitted as text because HTML need not be XML.

Raw HTML is literal text in both renderers. Markdown tag names cannot carry the `ac:` namespace, so storage-format macros are written with `--format storage` rather than embedded.

Task-list markers and other extensions stay disabled until their ADF and storage shapes are verified against the products.

## Do not add formatting features here unless they are in the Atlassian schema

New ADF node types should match Atlassian's published ADF spec. Don't invent node names. If an unfamiliar node type appears in real data, check the ADF docs first and handle it in `adf/blocks.rs` or `adf/inline.rs`.

## Cleanup is lossy by design

`confluence::cleanup` removes layout-only attributes and macro bookkeeping. This is intentional — users asking for markdown want readable text, not round-trippable HTML. Don't add round-trip support; use the default `--format storage` for that.
