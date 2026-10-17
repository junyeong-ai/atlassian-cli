//! Shared lexical handling for JQL and CQL filter injection.
//!
//! Tokens retain original byte positions. Quoted field names are identifiers
//! only at a clause boundary; quoted values never establish a search scope.

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word,
    Quoted,
    Open,
    Close,
    Operator,
    Other,
}

struct Token<'a> {
    kind: Kind,
    text: &'a str,
    start: usize,
}

impl Token<'_> {
    fn keyword(&self, keyword: &str) -> bool {
        self.kind == Kind::Word && self.text.eq_ignore_ascii_case(keyword)
    }

    fn field(&self, keyword: &str) -> bool {
        let name = match self.kind {
            Kind::Word => self.text,
            Kind::Quoted => &self.text[1..self.text.len() - 1],
            _ => return false,
        };
        name.eq_ignore_ascii_case(keyword)
            || (keyword == "space"
                && name.split_once('.').is_some_and(|(root, suffix)| {
                    root.eq_ignore_ascii_case(keyword)
                        && !suffix.is_empty()
                        && suffix.chars().all(|c| c.is_alphanumeric() || c == '_')
                }))
    }
}

fn tokens(query: &str) -> Vec<Token<'_>> {
    let bytes = query.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() {
            index += 1;
            continue;
        }
        let start = index;
        let first = bytes[index];
        index += 1;
        let kind = match first {
            b'(' => Kind::Open,
            b')' => Kind::Close,
            b',' => Kind::Other,
            b'\'' | b'"' => {
                let mut closed = false;
                while index < bytes.len() {
                    let byte = bytes[index];
                    index += 1;
                    if byte == b'\\' && index < bytes.len() {
                        index += 1;
                    } else if byte == first {
                        closed = true;
                        break;
                    }
                }
                if closed { Kind::Quoted } else { Kind::Other }
            }
            b'=' | b'!' | b'<' | b'>' | b'~' | b'&' | b'|' => {
                if bytes.get(start..start + 2).is_some_and(|pair| {
                    matches!(pair, b"!=" | b"!~" | b"<=" | b">=" | b"&&" | b"||")
                }) {
                    index += 1;
                }
                Kind::Operator
            }
            _ => {
                while index < bytes.len()
                    && !bytes[index].is_ascii_whitespace()
                    && !matches!(
                        bytes[index],
                        b'(' | b')'
                            | b','
                            | b'\''
                            | b'"'
                            | b'='
                            | b'!'
                            | b'<'
                            | b'>'
                            | b'~'
                            | b'&'
                            | b'|'
                    )
                {
                    index += 1;
                }
                Kind::Word
            }
        };
        tokens.push(Token {
            kind,
            text: &query[start..index],
            start,
        });
    }
    tokens
}

fn order_by_index(tokens: &[Token<'_>]) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            Kind::Open => depth += 1,
            Kind::Close => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth == 0
            && token.keyword("order")
            && tokens.get(index + 1).is_some_and(|next| next.keyword("by"))
        {
            return Some(index);
        }
    }
    None
}

fn is_field_operator(tokens: &[Token<'_>]) -> bool {
    let Some(first) = tokens.first() else {
        return false;
    };
    match first.kind {
        Kind::Operator => matches!(
            first.text,
            "=" | "!=" | "~" | "!~" | "<" | ">" | "<=" | ">="
        ),
        Kind::Word => {
            ["in", "is", "was", "changed"]
                .iter()
                .any(|word| first.keyword(word))
                || (first.keyword("not") && tokens.get(1).is_some_and(|next| next.keyword("in")))
        }
        _ => false,
    }
}

fn has_scope(tokens: &[Token<'_>], keyword: &str) -> bool {
    let mut field_position = true;
    let mut value_depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        if value_depth > 0 {
            match token.kind {
                Kind::Open => value_depth += 1,
                Kind::Close => value_depth -= 1,
                _ => {}
            }
            continue;
        }
        if field_position {
            if token.kind == Kind::Open
                || token.keyword("not")
                || (token.kind == Kind::Operator && token.text == "!")
            {
                continue;
            }
            if token.field(keyword) && is_field_operator(&tokens[index + 1..]) {
                return true;
            }
            field_position = false;
        } else if token.kind == Kind::Open {
            value_depth = 1;
        } else if token.keyword("and")
            || token.keyword("or")
            || (token.kind == Kind::Operator && matches!(token.text, "&" | "&&" | "|" | "||"))
        {
            field_position = true;
        }
    }
    false
}

/// Add a default scope unless a condition explicitly names that field.
/// Preserve the original sorting clause and leave syntax validation to Atlassian.
pub(crate) fn inject_filter(query: &str, keyword: &str, injected_clause: &str) -> String {
    let tokens = tokens(query);
    let order = order_by_index(&tokens);
    let conditions = &tokens[..order.unwrap_or(tokens.len())];
    if has_scope(conditions, keyword) {
        return query.to_string();
    }
    let split = order.map(|index| {
        query[..tokens[index].start]
            .trim_end_matches(|c: char| c.is_ascii_whitespace())
            .len()
    });
    let body = query[..split.unwrap_or(query.len())].trim();
    let base = if body.is_empty() {
        injected_clause.to_string()
    } else {
        format!("{injected_clause} AND ({body})")
    };
    match split.map(|index| &query[index..]) {
        Some(suffix) if suffix.starts_with(|c: char| c.is_ascii_whitespace()) => {
            format!("{base}{suffix}")
        }
        Some(suffix) => format!("{base} {suffix}"),
        None => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const FILTER: &str = "project IN (\"MDW\")";

    #[test]
    fn explicit_scope_conditions_preserve_the_query() {
        for query in [
            "project = OTHER",
            "status = Open&&project = OTHER",
            "status = Open&&!project = OTHER",
            "status = Open||!(project = OTHER)",
            "status = Open&project = OTHER",
            "status = Open|project = OTHER",
            "status = Open||\"project\" = OTHER",
            "!project = OTHER",
            "!(\"project\" = OTHER)",
            "PrOjEcT=OTHER",
            "\"project\" = OTHER",
            "'PROJECT' != OTHER",
            "NOT(\"project\" IN(OTHER))",
            "status = Open OR ('project' = OTHER AND status != Done)",
            "project ~ X",
            "project !~ X",
            "project <= X",
            "project >= X",
            "project in(A)",
            "project NOT IN (A)",
            "project IS NOT EMPTY",
            "project WAS OLD",
            "project CHANGED",
            "project = OTHER\nORDER\tBY \"Sort field\" DESC",
        ] {
            assert_eq!(inject_filter(query, "project", FILTER), query, "{query}");
        }
        for query in [
            "space.key = ENG",
            "'space' = ENG",
            "\"space.key\" in (ENG)",
            "space.type = global",
            "space.title ~ \"Engineering\"",
        ] {
            assert_eq!(
                inject_filter(query, "space", "space IN (\"DEFAULT\")"),
                query
            );
        }
    }

    #[test]
    fn values_and_function_arguments_never_establish_a_scope() {
        for query in [
            "summary ~ \"project = OTHER\"",
            "summary ~ 'project = OTHER'",
            "summary ~ \"it's project = OTHER\"",
            "summary ~ 'say \"project = OTHER\"'",
            "summary ~ \"escaped \\\" project = OTHER\"",
            "summary ~ \"project\"",
            "summary ~ project",
            "field = function(\"project\", \"AND\")",
            "field in (project, other)",
            "projectId = 10500",
            "myproject = OTHER",
            "project.foo = OTHER",
            "\"project.key\" = OTHER",
            "field = function(ORDER BY created)",
            "field = preorder BY created",
            "field = foo.order BY created",
            "field = ORDERBY created",
            "field = ORDER BYfield",
            "field = ORDER \"ignored\" BY created",
            "summary ~ \"unterminated project = OTHER",
        ] {
            assert_eq!(
                inject_filter(query, "project", FILTER),
                format!("{FILTER} AND ({query})"),
                "{query}"
            );
        }
        for query in [
            "title ~ space.shuttle",
            "label = space.station",
            "spaceKey = ENG",
        ] {
            assert_eq!(
                inject_filter(query, "space", "space IN (\"DEFAULT\")"),
                format!("space IN (\"DEFAULT\") AND ({query})")
            );
        }
    }

    #[test]
    fn sorting_preserves_unicode_and_token_boundaries() {
        for (query, expected) in [
            (
                "labels = K ORDER BY created",
                "project IN (\"MDW\") AND (labels = K) ORDER BY created",
            ),
            (
                "labels = İ ORDER BY created",
                "project IN (\"MDW\") AND (labels = İ) ORDER BY created",
            ),
            (
                "status = Open\nORDER\tBY created DESC",
                "project IN (\"MDW\") AND (status = Open)\nORDER\tBY created DESC",
            ),
            (
                "status = Open\r\nOrDeR  bY \"Custom field\", created DESC",
                "project IN (\"MDW\") AND (status = Open)\r\nOrDeR  bY \"Custom field\", created DESC",
            ),
            (
                "status = Open ORDER BY\"created\"",
                "project IN (\"MDW\") AND (status = Open) ORDER BY\"created\"",
            ),
            (
                "status = Open ORDER BY'created'",
                "project IN (\"MDW\") AND (status = Open) ORDER BY'created'",
            ),
            (
                "(status = Open)ORDER BY created",
                "project IN (\"MDW\") AND ((status = Open)) ORDER BY created",
            ),
            (
                "status = \"Open\"ORDER BY created",
                "project IN (\"MDW\") AND (status = \"Open\") ORDER BY created",
            ),
            (
                " \tORDER\nBY created",
                "project IN (\"MDW\") \tORDER\nBY created",
            ),
        ] {
            assert_eq!(inject_filter(query, "project", FILTER), expected, "{query}");
        }
    }

    #[test]
    fn quoted_sorting_and_scope_names_remain_values() {
        for query in [
            "summary ~ \"ORDER BY created\"",
            "summary ~ 'ORDER BY created'",
            "field = \"order\" BY created",
        ] {
            assert_eq!(
                inject_filter(query, "project", FILTER),
                format!("{FILTER} AND ({query})")
            );
        }
        assert_eq!(
            inject_filter("status = Open ORDER BY \"project\"", "project", FILTER),
            "project IN (\"MDW\") AND (status = Open) ORDER BY \"project\""
        );
    }

    #[test]
    fn empty_and_sort_only_queries_have_no_condition_group() {
        for query in ["", " ", "\t\r\n"] {
            assert_eq!(inject_filter(query, "project", FILTER), FILTER);
        }
        assert_eq!(
            inject_filter("ORDER BY created DESC", "project", FILTER),
            "project IN (\"MDW\") ORDER BY created DESC"
        );
    }
}
