//! Extracting load-bearing fields from a successful write's response body.
//!
//! A 2xx write whose body lacks the field a follow-up command needs (the new
//! `id`, `key`, or `version`) means the server returned an unexpected shape —
//! schema drift, an error page served with a 200, or a proxy that dropped the
//! body. Substituting a placeholder `null` would hand a JSON-first caller a
//! resource identifier that does not exist, so it chains a confusing 404 far
//! from the real cause. Every write therefore extracts its identifiers through
//! [`require_field`] (string/structured ids) or [`require_u64`] (counts and
//! version numbers), which fail loud — the same posture the pagination helpers
//! take when `values`/`results` is missing.

use anyhow::Result;
use serde_json::Value;

/// Read a required string-or-structured field from a write's parsed response
/// body by JSON Pointer (e.g. `/id`, `/key`, `/results/0/id`), returning it by
/// value. Bails when the field is absent or `null`, naming the operation and
/// echoing the body so the unexpected shape is diagnosable.
pub(crate) fn require_field(body: &Value, pointer: &str, operation: &str) -> Result<Value> {
    match body.pointer(pointer) {
        Some(value) if !value.is_null() => Ok(value.clone()),
        _ => anyhow::bail!(
            "{operation} succeeded but its response had no '{}': {body}",
            pointer.trim_start_matches('/')
        ),
    }
}

/// Read a required numeric field (a count or version number) by JSON Pointer,
/// returning it as `u64`. Bails when the field is absent, `null`, or not an
/// integer, so the envelope's numeric shape can never silently degrade to a
/// string the way a raw passthrough would.
pub(crate) fn require_u64(body: &Value, pointer: &str, operation: &str) -> Result<u64> {
    body.pointer(pointer)
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{operation} succeeded but '{}' was absent or not an integer: {body}",
                pointer.trim_start_matches('/')
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn returns_present_field() {
        let body = json!({ "id": "10", "key": "ABC-1" });
        assert_eq!(
            require_field(&body, "/key", "create issue").unwrap(),
            json!("ABC-1")
        );
    }

    #[test]
    fn follows_nested_and_indexed_pointers() {
        let body = json!({ "version": { "number": 3 }, "results": [{ "id": "att1" }] });
        assert_eq!(
            require_field(&body, "/version/number", "update page").unwrap(),
            json!(3)
        );
        assert_eq!(
            require_field(&body, "/results/0/id", "upload").unwrap(),
            json!("att1")
        );
    }

    #[test]
    fn bails_on_missing_field() {
        let body = json!({ "title": "no id here" });
        let err = require_field(&body, "/id", "create page")
            .unwrap_err()
            .to_string();
        assert!(err.contains("create page succeeded but its response had no 'id'"));
        assert!(err.contains("no id here"));
    }

    #[test]
    fn bails_on_null_field() {
        let body = json!({ "id": null });
        assert!(require_field(&body, "/id", "add comment").is_err());
    }

    #[test]
    fn require_u64_extracts_and_rejects_non_integers() {
        let body = json!({ "totalSize": 42, "version": { "number": 3 } });
        assert_eq!(require_u64(&body, "/totalSize", "search").unwrap(), 42);
        assert_eq!(
            require_u64(&body, "/version/number", "update page").unwrap(),
            3
        );
        // A stringified number is a shape drift, not a valid count.
        assert!(require_u64(&json!({ "totalSize": "42" }), "/totalSize", "search").is_err());
        assert!(require_u64(&json!({}), "/totalSize", "search").is_err());
    }
}
