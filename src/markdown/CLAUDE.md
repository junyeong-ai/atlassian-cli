# markdown module

## Two converters, different pipelines

- `adf::adf_to_markdown` — walks Atlassian Document Format JSON (Jira). Handles `paragraph`, `heading`, `bulletList`, `orderedList`, `codeBlock`, `panel`, `table`, `mention`, `inlineCard`, marks, etc. Unknown node types are skipped, not errored, so schema additions upstream don't break reads.
- `confluence::confluence_to_markdown` — runs `htmd` on HTML storage format. Confluence HTML includes `ac:*` / `ri:*` namespaced tags and macro blocks; `confluence::cleanup` strips residue (schema-version, mxgraph base64, etc.) before conversion.

## Do not add formatting features here unless they are in the Atlassian schema

New ADF node types should match Atlassian's published ADF spec. Don't invent node names. If an unfamiliar node type appears in real data, check the ADF docs first and handle it in `adf/blocks.rs` or `adf/inline.rs`.

## Cleanup is lossy by design

`confluence::cleanup` removes layout-only attributes and macro bookkeeping. This is intentional — users asking for markdown want readable text, not round-trippable HTML. Don't add round-trip support; use `--format html` (default) for that.
