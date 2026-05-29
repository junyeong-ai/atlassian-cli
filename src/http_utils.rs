//! Cross-cutting HTTP helpers shared by the Jira and Confluence API layers.

use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};

/// Complement of RFC 3986 `pchar` (`unreserved / pct-encoded / sub-delims /
/// ":" / "@"`) — every byte we must percent-encode when interpolating user
/// input into a URL path segment.
///
/// All `gen-delims` (`:`, `/`, `?`, `#`, `[`, `]`, `@`) other than `:` and
/// `@` are encoded; `:` and `@` are technically legal inside a path segment
/// but we still encode them defensively because Atlassian path segments
/// never contain them in practice and tolerating them invites injection
/// surprises. Sub-delims (`!$&'()*+,;=`) are pass-through because clap
/// accepts them only in benign forms.
pub(crate) const PATH_SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b':')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

/// Percent-encode a single URL path segment.
///
/// Use at every site that interpolates **user input** into a URL path.
/// Query parameters go through reqwest's `.query()` builder, which handles
/// its own encoding — do not chain this helper for query values.
///
/// Do not pass server-side identifiers (cloud IDs, numeric resource IDs
/// returned by the API) through this function. Cloud IDs in particular
/// embed `:` characters that the defensive AsciiSet would re-encode,
/// breaking the proxy URLs that `BasicStrategy::build_url` and
/// `proxy_url` construct. Those are already safe by construction —
/// encode only what the user typed.
pub(crate) fn encode_path_segment(segment: &str) -> String {
    utf8_percent_encode(segment, PATH_SEGMENT).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_through_safe_chars() {
        assert_eq!(encode_path_segment("MDW-207"), "MDW-207");
        assert_eq!(encode_path_segment("abc_123.def~ghi"), "abc_123.def~ghi");
    }

    #[test]
    fn encodes_space() {
        assert_eq!(encode_path_segment("MDW 207"), "MDW%20207");
    }

    #[test]
    fn encodes_slash() {
        assert_eq!(encode_path_segment("a/b"), "a%2Fb");
    }

    #[test]
    fn encodes_brackets() {
        // RFC 3986 gen-delims — must be encoded inside path segments to avoid
        // ambiguity with IPv6 host syntax in other URL components.
        assert_eq!(encode_path_segment("a[1]"), "a%5B1%5D");
    }

    #[test]
    fn encodes_at_and_colon() {
        assert_eq!(encode_path_segment("u@h"), "u%40h");
        assert_eq!(encode_path_segment("a:b"), "a%3Ab");
    }

    #[test]
    fn encodes_query_delimiters() {
        assert_eq!(encode_path_segment("a?b"), "a%3Fb");
        assert_eq!(encode_path_segment("a#b"), "a%23b");
    }

    #[test]
    fn double_encodes_already_encoded_input() {
        // Defense in depth: we never trust callers to pre-encode. Passing an
        // already-encoded segment intentionally double-encodes so the raw
        // bytes round-trip unchanged on the server side.
        assert_eq!(encode_path_segment("MDW%20207"), "MDW%2520207");
    }

    #[test]
    fn encodes_unicode_bytes() {
        // utf8 multi-byte sequences are percent-encoded byte by byte.
        let encoded = encode_path_segment("café");
        assert_eq!(encoded, "caf%C3%A9");
    }

    #[test]
    fn empty_string() {
        assert_eq!(encode_path_segment(""), "");
    }
}
