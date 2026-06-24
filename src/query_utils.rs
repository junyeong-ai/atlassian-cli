//! Shared helpers for parsing Atlassian query languages (JQL, CQL).
//!
//! JQL and CQL share the same string-literal syntax (single- or double-quoted
//! with `\` escape) and the same need to detect bare keywords and `ORDER BY`
//! clauses without false-positives on text inside quoted literals. The filter
//! injection logic for both languages is identical apart from the keyword and
//! the wrapping clause, so it lives here once as `inject_filter`.

use regex::Regex;

/// Replace every quoted string literal in a query with a same-length run of
/// ASCII spaces. Byte offsets and overall length are preserved so the caller
/// can still slice the original input by indices found in the mask.
///
/// Used to defang user queries before running keyword-detection regexes — e.g.
/// so `summary ~ "project = foo"` does not trip the `\bproject\s*=` detector
/// and suppress the project filter we want to inject.
///
/// Both `'` and `"` open a literal; the closer must match the opener, so an
/// apostrophe inside a double-quoted value (`"it's fine"`) and a double-quote
/// inside a single-quoted value (`'say "hi"'`) are both handled correctly.
/// JQL and CQL both accept either quote form, so masking only one would let
/// `project = 'X'` clauses slip through unmasked. A `\` escapes the next byte.
pub(crate) fn mask_string_literals(query: &str) -> String {
    let bytes = query.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' || b == b'\'' {
            let opener = b;
            out.push(b' ');
            i += 1;
            while i < bytes.len() {
                let c = bytes[i];
                if c == b'\\' && i + 1 < bytes.len() {
                    out.push(b' ');
                    out.push(b' ');
                    i += 2;
                    continue;
                }
                if c == opener {
                    out.push(b' ');
                    i += 1;
                    break;
                }
                out.push(b' ');
                i += 1;
            }
        } else {
            out.push(b);
            i += 1;
        }
    }
    // Only ASCII bytes are ever substituted (spaces for quoted content). Bytes
    // outside the masked regions are copied verbatim, so a valid UTF-8 input
    // yields a valid UTF-8 output.
    String::from_utf8(out).expect("mask preserves UTF-8")
}

/// Build the regex that decides whether a query already scopes by `keyword`
/// (`project` for JQL, `space` for CQL) — when it does, the configured filter
/// is not injected on top. The keyword (optionally a dotted sub-field like
/// `space.key`/`space.type`) must be followed by a field operator: comparison
/// (`=`, `!=`, `~`, `!~`, `<`, `>`), set membership (`in`, `not in`), or
/// existence/history (`is`, `was`, `changed` — JQL only; harmless for CQL).
/// Requiring an operator is what keeps a bare value like `title ~ space.foo`
/// from counting as a scope, while still catching `project WAS "X"` and
/// `space.key = Y`. The leading/trailing word boundary stops `projectId`/
/// `spaceKey` from matching.
pub(crate) fn clause_detector(keyword: &str) -> Regex {
    Regex::new(&format!(
        r"(?i)\b{keyword}\b(?:\.\w+)?\s*(?:[=!<>~]|\b(?:in|not\s+in|is|was|changed)\b)"
    ))
    .expect("clause detector pattern is valid")
}

/// Inject a filter clause into a user query (JQL or CQL), preserving any
/// trailing `ORDER BY` and skipping injection when the user already scoped by
/// the same keyword. This is the single source of truth shared by Jira's
/// project filter and Confluence's space filter — the only differences between
/// the two languages are `clause_re` (the existing-clause detector) and
/// `injected_clause` (e.g. `project IN ("A")` or `space IN ("S")`).
///
/// - Quoted literals are masked before detection so quoted text never trips
///   `clause_re` or the `ORDER BY` split.
/// - When the user's conditions already match `clause_re`, the original query
///   is returned untouched.
/// - An empty/whitespace condition body collapses to just the injected clause
///   (no dangling `AND ()`), with any `ORDER BY` appended after it.
pub(crate) fn inject_filter(query: &str, clause_re: &Regex, injected_clause: &str) -> String {
    let mask = mask_string_literals(query);
    let mask_lower = mask.to_lowercase();

    let (conditions, order_by) = if let Some(pos) = mask_lower.find(" order by ") {
        (query[..pos].to_string(), Some(query[pos..].to_string()))
    } else if mask_lower.starts_with("order by ") {
        (String::new(), Some(format!(" {}", query)))
    } else {
        (query.to_string(), None)
    };

    // Detect an existing clause against the masked conditions so quoted text
    // like `summary ~ "project = foo"` does not count.
    let condition_mask = mask_string_literals(&conditions);
    if clause_re.is_match(&condition_mask) {
        return query.to_string();
    }

    let trimmed = conditions.trim();
    let base = if trimmed.is_empty() {
        injected_clause.to_string()
    } else {
        format!("{} AND ({})", injected_clause, trimmed)
    };

    match order_by {
        Some(order_clause) => format!("{}{}", base, order_clause),
        None => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_length_and_strips_contents() {
        let input = "a = \"hello\" and b = 1";
        let masked = mask_string_literals(input);
        assert_eq!(masked.len(), input.len());
        assert!(!masked.contains("hello"));
        assert!(masked.contains(" and b = 1"));
    }

    #[test]
    fn handles_escaped_quote() {
        let input = "summary ~ \"he said \\\"hi\\\"\" and project = X";
        let masked = mask_string_literals(input);
        assert_eq!(masked.len(), input.len());
        assert!(masked.contains("and project = X"));
    }

    #[test]
    fn leaves_unquoted_text_intact() {
        let input = "project = MDW AND status = Open";
        assert_eq!(mask_string_literals(input), input);
    }

    #[test]
    fn preserves_byte_offsets_for_multi_byte_content_inside_quotes() {
        // The masker replaces multi-byte UTF-8 bytes inside a quoted region
        // with same-length spaces. Length must remain identical.
        let input = "summary ~ \"café\" and project = X";
        let masked = mask_string_literals(input);
        assert_eq!(masked.len(), input.len());
        assert!(masked.contains("and project = X"));
    }

    #[test]
    fn masks_single_quoted_literals() {
        // JQL/CQL both accept single-quoted strings. `project = 'foo'` content
        // must be masked so a keyword inside it can't trip clause detection.
        let input = "project = 'My Project' AND status = Open";
        let masked = mask_string_literals(input);
        assert_eq!(masked.len(), input.len());
        assert!(!masked.contains("My Project"));
        assert!(masked.contains("AND status = Open"));
    }

    #[test]
    fn apostrophe_inside_double_quotes_does_not_close() {
        // The closer must match the opener — an apostrophe inside `"..."` is
        // literal content, not a delimiter.
        let input = "summary ~ \"it's fine\" AND project = X";
        let masked = mask_string_literals(input);
        assert_eq!(masked.len(), input.len());
        assert!(masked.contains("AND project = X"));
        assert!(!masked.contains("it's fine"));
    }

    #[test]
    fn double_quote_inside_single_quotes_does_not_close() {
        let input = "summary ~ 'say \"hi\"' AND project = X";
        let masked = mask_string_literals(input);
        assert_eq!(masked.len(), input.len());
        assert!(masked.contains("AND project = X"));
    }

    #[test]
    fn unmatched_quote_runs_to_end_of_string() {
        // We could bail here, but server-side parsing will reject the JQL
        // first and surface a clear error — local truncation is acceptable.
        let input = "summary = \"unterminated";
        let masked = mask_string_literals(input);
        assert_eq!(masked.len(), input.len());
        assert!(masked.starts_with("summary = "));
    }

    #[test]
    fn empty_input() {
        assert_eq!(mask_string_literals(""), "");
    }

    #[test]
    fn handles_escaped_backslash() {
        // `\\` inside a quoted segment must consume both bytes as a single
        // escape pair, not terminate the string at the first backslash.
        let input = "summary ~ \"a \\\\ b\" and project = X";
        let masked = mask_string_literals(input);
        assert_eq!(masked.len(), input.len());
        assert!(masked.contains("and project = X"));
    }

    #[test]
    fn trailing_backslash_at_end_of_quoted_string() {
        // A backslash as the final byte of a quoted region without a
        // following character must not advance past end-of-string.
        let input = "summary ~ \"abc\\";
        let masked = mask_string_literals(input);
        assert_eq!(masked.len(), input.len());
    }

    #[test]
    fn empty_quoted_string() {
        let input = "a = \"\" and b = 1";
        let masked = mask_string_literals(input);
        assert_eq!(masked.len(), input.len());
        assert!(masked.contains(" and b = 1"));
    }

    #[test]
    fn multiple_quoted_segments_preserve_unquoted_between() {
        let input = "a = \"x\" AND b = \"y\" AND project = MDW";
        let masked = mask_string_literals(input);
        assert_eq!(masked.len(), input.len());
        assert!(masked.contains("AND project = MDW"));
    }

    fn project_re() -> Regex {
        clause_detector("project")
    }

    #[test]
    fn clause_detector_matches_all_field_operators() {
        let re = project_re();
        for q in [
            "project = MDW",
            "project=MDW",
            "project != X",
            "project ~ X",
            "project in (A, B)",
            "project IN(A)",
            "project not in (A)",
            "project is empty",
            "project IS NOT EMPTY",
            "project was OLD",
            "project changed",
        ] {
            assert!(re.is_match(q), "should detect scope in: {q}");
        }
    }

    #[test]
    fn clause_detector_matches_dotted_subfields() {
        // CQL `space.key`/`space.type` are scopes; the dotted form must count.
        let re = clause_detector("space");
        assert!(re.is_match("space.key = ENG"));
        assert!(re.is_match("space.type = global"));
        assert!(re.is_match("space = ENG"));
    }

    #[test]
    fn clause_detector_ignores_lookalike_identifiers() {
        // A word boundary keeps `projectId`/`spaceKey` from counting as a scope.
        let re = project_re();
        assert!(!re.is_match("projectId = 10500"));
        assert!(!clause_detector("space").is_match("spaceKey = X"));
        // The bare keyword with no operator is not a scope either.
        assert!(!re.is_match("summary ~ project"));
    }

    #[test]
    fn clause_detector_ignores_dotted_keyword_used_as_a_value() {
        // `space.foo` as an unquoted RHS value is not a scope — only a dotted
        // sub-field *followed by an operator* (`space.key =`) counts.
        let re = clause_detector("space");
        assert!(!re.is_match("title ~ space.shuttle"));
        assert!(!re.is_match("label = space.station"));
        assert!(re.is_match("space.key in (ENG, OPS)"));
    }

    #[test]
    fn inject_filter_skips_on_history_and_existence_clauses() {
        // Regression guard: a `WAS`/`IS EMPTY` scope must suppress injection so
        // the configured filter can't clobber the user's explicit scoping.
        let re = project_re();
        assert_eq!(
            inject_filter("project WAS \"OLD\"", &re, "project IN (\"MDW\")"),
            "project WAS \"OLD\""
        );
        assert_eq!(
            inject_filter("project IS EMPTY", &re, "project IN (\"MDW\")"),
            "project IS EMPTY"
        );
    }

    #[test]
    fn inject_filter_wraps_bare_conditions() {
        let re = project_re();
        let out = inject_filter("status = Open", &re, "project IN (\"MDW\")");
        assert_eq!(out, "project IN (\"MDW\") AND (status = Open)");
    }

    #[test]
    fn inject_filter_skips_when_clause_present() {
        let re = project_re();
        let out = inject_filter("project = X AND status = Open", &re, "project IN (\"MDW\")");
        assert_eq!(out, "project = X AND status = Open");
    }

    #[test]
    fn inject_filter_skips_on_single_quoted_clause() {
        // Regression guard for the round-3 single-quote miss: a clause using
        // single quotes must still be detected and suppress injection.
        let re = project_re();
        let out = inject_filter("project = 'My Project'", &re, "project IN (\"MDW\")");
        assert_eq!(out, "project = 'My Project'");
    }

    #[test]
    fn inject_filter_preserves_trailing_order_by() {
        let re = project_re();
        let out = inject_filter(
            "status = Open ORDER BY created DESC",
            &re,
            "project IN (\"MDW\")",
        );
        assert_eq!(
            out,
            "project IN (\"MDW\") AND (status = Open) ORDER BY created DESC"
        );
    }

    #[test]
    fn inject_filter_order_by_only_collapses_conditions() {
        let re = project_re();
        let out = inject_filter("ORDER BY created DESC", &re, "project IN (\"MDW\")");
        assert_eq!(out, "project IN (\"MDW\") ORDER BY created DESC");
    }

    #[test]
    fn inject_filter_empty_query_yields_bare_clause() {
        let re = project_re();
        assert_eq!(
            inject_filter("   ", &re, "project IN (\"MDW\")"),
            "project IN (\"MDW\")"
        );
    }

    #[test]
    fn inject_filter_ignores_quoted_order_by() {
        let re = project_re();
        // `order by` inside a quoted literal must not split the query.
        let out = inject_filter(
            "summary ~ \"finish order by tomorrow\"",
            &re,
            "project IN (\"MDW\")",
        );
        assert_eq!(
            out,
            "project IN (\"MDW\") AND (summary ~ \"finish order by tomorrow\")"
        );
    }
}
