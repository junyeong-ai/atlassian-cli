use serde::{Deserialize, Serialize};

/// Authentication configuration.
///
/// Uses serde tagged enum with `method` field for explicit auth method selection.
/// No heuristic-based auto-detection — the method must be explicitly declared.
///
/// TOML examples:
/// ```toml
/// [default.auth]
/// method = "basic"
/// email = "user@example.com"
/// token = "api-token"
///
/// [default.auth]
/// method = "service_account"
/// client_id = "your-client-id"
/// client_secret = "your-secret"
/// cloud_id = "optional-cloud-id"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "lowercase", deny_unknown_fields)]
pub enum AuthConfig {
    Basic {
        /// Defaults to "" when omitted in config file (overridden by env var or CLI flag).
        #[serde(default)]
        email: String,
        /// Defaults to "" when omitted in config file (overridden by env var or CLI flag).
        #[serde(default, skip_serializing)]
        token: String,
    },
    #[serde(rename = "service_account")]
    ServiceAccount {
        /// Defaults to "" when omitted in config file (overridden by env var or CLI flag).
        #[serde(default)]
        client_id: String,
        /// Defaults to "" when omitted in config file (overridden by env var or CLI flag).
        #[serde(default, skip_serializing)]
        client_secret: String,
        /// Auto-discovered via accessible-resources API when None.
        cloud_id: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_auth_deserialization() {
        let toml_str = r#"
            method = "basic"
            email = "user@example.com"
            token = "my-token"
        "#;
        let auth: AuthConfig = toml::from_str(toml_str).unwrap();
        match auth {
            AuthConfig::Basic { email, token } => {
                assert_eq!(email, "user@example.com");
                assert_eq!(token, "my-token");
            }
            _ => panic!("Expected Basic auth"),
        }
    }

    #[test]
    fn test_service_account_deserialization() {
        let toml_str = r#"
            method = "service_account"
            client_id = "test-client-id"
            client_secret = "test-secret"
        "#;
        let auth: AuthConfig = toml::from_str(toml_str).unwrap();
        match auth {
            AuthConfig::ServiceAccount {
                client_id,
                client_secret,
                cloud_id,
            } => {
                assert_eq!(client_id, "test-client-id");
                assert_eq!(client_secret, "test-secret");
                assert!(cloud_id.is_none());
            }
            _ => panic!("Expected Service account auth"),
        }
    }

    #[test]
    fn test_service_account_with_cloud_id() {
        let toml_str = r#"
            method = "service_account"
            client_id = "cid"
            client_secret = "secret"
            cloud_id = "abc-123"
        "#;
        let auth: AuthConfig = toml::from_str(toml_str).unwrap();
        match auth {
            AuthConfig::ServiceAccount { cloud_id, .. } => {
                assert_eq!(cloud_id, Some("abc-123".to_string()));
            }
            _ => panic!("Expected Service account auth"),
        }
    }

    #[test]
    fn test_basic_serialization_skips_token() {
        let auth = AuthConfig::Basic {
            email: "user@test.com".to_string(),
            token: "secret-token".to_string(),
        };
        let serialized = toml::to_string(&auth).unwrap();
        assert!(serialized.contains("email"));
        assert!(!serialized.contains("secret-token"));
    }

    #[test]
    fn test_service_account_serialization_skips_secret() {
        let auth = AuthConfig::ServiceAccount {
            client_id: "cid".to_string(),
            client_secret: "secret".to_string(),
            cloud_id: None,
        };
        let serialized = toml::to_string(&auth).unwrap();
        assert!(serialized.contains("method = \"service_account\""));
        assert!(serialized.contains("client_id"));
        assert!(!serialized.contains("secret"));
    }

    #[test]
    fn test_basic_partial_config_deserializes() {
        // User may specify email in config file but token via env var.
        // token field must default to "" (not fail to parse).
        let toml_str = r#"
            method = "basic"
            email = "user@example.com"
        "#;
        let auth: AuthConfig = toml::from_str(toml_str).unwrap();
        match auth {
            AuthConfig::Basic { email, token } => {
                assert_eq!(email, "user@example.com");
                assert_eq!(token, ""); // empty, to be filled by env var
            }
            _ => panic!("Expected Basic auth"),
        }
    }

    #[test]
    fn test_service_account_partial_config_deserializes() {
        // client_secret may come from env var only
        let toml_str = r#"
            method = "service_account"
            client_id = "cid"
        "#;
        let auth: AuthConfig = toml::from_str(toml_str).unwrap();
        match auth {
            AuthConfig::ServiceAccount {
                client_id,
                client_secret,
                ..
            } => {
                assert_eq!(client_id, "cid");
                assert_eq!(client_secret, ""); // to be filled by env var
            }
            _ => panic!("Expected Service account auth"),
        }
    }

    #[test]
    fn test_basic_method_only_deserializes() {
        // Extreme case: only method specified, everything from env vars
        let toml_str = r#"method = "basic""#;
        let auth: AuthConfig = toml::from_str(toml_str).unwrap();
        match auth {
            AuthConfig::Basic { email, token } => {
                assert_eq!(email, "");
                assert_eq!(token, "");
            }
            _ => panic!("Expected Basic auth"),
        }
    }

    #[test]
    fn test_invalid_method_fails() {
        let toml_str = r#"
            method = "unknown"
            email = "user@example.com"
        "#;
        let result: Result<AuthConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }
}
