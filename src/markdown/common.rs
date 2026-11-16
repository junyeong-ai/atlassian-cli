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

/// A fenced code block whose fence no backtick run in `code` can close early.
/// A language holding a backtick would void a backtick fence, so it is dropped.
pub fn fenced_code_block(language: &str, code: &str) -> String {
    let longest_run = code.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest_run.max(2) + 1);
    let language = if language.contains('`') { "" } else { language };
    let code = code.trim_end_matches(['\n', '\r']);
    format!("{fence}{language}\n{code}\n{fence}")
}
