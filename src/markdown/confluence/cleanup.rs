use regex::Regex;
use std::sync::LazyLock;

static MACRO_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\s*ac:macro-id="[^"]*""#).unwrap());

static SCHEMA_VERSION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\s*ac:schema-version="[^"]*""#).unwrap());

static DATA_LAYOUT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\s*data-layout="[^"]*""#).unwrap());

static EMPTY_PARAM_SELF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<ac:parameter ac:name=""\s*/>"#).unwrap());

static EMPTY_PARAM_PAIR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<ac:parameter ac:name="">[^<]*</ac:parameter>"#).unwrap());

static CDATA_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<!\[CDATA\[([\s\S]*?)\]\]>"#).unwrap());

static ADF_ATTRIBUTE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<ac:adf-attribute[^>]*>.*?</ac:adf-attribute>"#).unwrap());

static MX_GRAPH_MODEL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<mxGraphModel[\s\S]*?</mxGraphModel>"#).unwrap());

static MX_FILE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<mxfile[\s\S]*?</mxfile>"#).unwrap());

static LONG_BASE64_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"[A-Za-z0-9+/=]{500,}"#).unwrap());

pub fn clean_metadata(html: &str) -> String {
    let mut result = html.to_string();

    result = MACRO_ID_RE.replace_all(&result, "").to_string();
    result = SCHEMA_VERSION_RE.replace_all(&result, "").to_string();
    result = DATA_LAYOUT_RE.replace_all(&result, "").to_string();
    result = EMPTY_PARAM_SELF_RE.replace_all(&result, "").to_string();
    result = EMPTY_PARAM_PAIR_RE.replace_all(&result, "").to_string();
    result = ADF_ATTRIBUTE_RE.replace_all(&result, "").to_string();
    result = CDATA_RE.replace_all(&result, "$1").to_string();

    result
}

pub fn clean_binary_data(content: &str) -> String {
    let mut result = content.to_string();

    result = MX_GRAPH_MODEL_RE.replace_all(&result, "").to_string();
    result = MX_FILE_RE.replace_all(&result, "").to_string();
    result = LONG_BASE64_RE.replace_all(&result, "").to_string();

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remove_macro_id() {
        let html = r#"<ac:structured-macro ac:name="code" ac:macro-id="abc-123">"#;
        let result = clean_metadata(html);
        assert!(!result.contains("ac:macro-id"));
        assert!(result.contains("ac:name=\"code\""));
    }

    #[test]
    fn test_remove_schema_version() {
        let html = r#"<ac:structured-macro ac:name="code" ac:schema-version="1">"#;
        let result = clean_metadata(html);
        assert!(!result.contains("ac:schema-version"));
    }

    #[test]
    fn test_unwrap_cdata() {
        let html = r#"<![CDATA[let x = 1;]]>"#;
        let result = clean_metadata(html);
        assert_eq!(result, "let x = 1;");
    }

    #[test]
    fn test_remove_empty_params() {
        let html = r#"<ac:parameter ac:name="" /><ac:parameter ac:name="">value</ac:parameter>"#;
        let result = clean_metadata(html);
        assert!(!result.contains("ac:parameter"));
    }

    #[test]
    fn test_remove_adf_attribute() {
        let html = r#"<ac:adf-attribute key="panel-type">note</ac:adf-attribute>"#;
        let result = clean_metadata(html);
        assert!(result.is_empty());
    }

    #[test]
    fn test_remove_mxgraphmodel() {
        let content = r#"<mxGraphModel><root></root></mxGraphModel> text"#;
        let result = clean_binary_data(content);
        assert_eq!(result, "text");
    }

    #[test]
    fn test_remove_long_base64() {
        let base64 = "A".repeat(600);
        let content = format!("before {} after", base64);
        let result = clean_binary_data(&content);
        assert_eq!(result, "before  after");
    }
}
