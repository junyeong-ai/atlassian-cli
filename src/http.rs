use crate::config::Config;
use reqwest::Client;
use std::time::Duration;

pub fn client(config: &Config) -> Client {
    Client::builder()
        .timeout(Duration::from_millis(config.performance.request_timeout_ms))
        .build()
        .expect("Failed to create HTTP client")
}

pub fn auth_header(config: &Config) -> String {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let credentials = format!("{}:{}", config.email(), config.token());
    format!("Basic {}", STANDARD.encode(credentials))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::create_test_config;

    #[test]
    fn test_client_creation() {
        let config = create_test_config();
        let client = client(&config);
        assert!(format!("{:?}", client).contains("Client"));
    }

    #[test]
    fn test_auth_header_format() {
        let config = create_test_config();
        let header = auth_header(&config);
        assert!(header.starts_with("Basic "));
    }

    #[test]
    fn test_auth_header_encoding() {
        use base64::{Engine as _, engine::general_purpose::STANDARD};

        let config = create_test_config();
        let header = auth_header(&config);
        let base64_part = &header[6..];
        let decoded = STANDARD.decode(base64_part).unwrap();
        let credentials = String::from_utf8(decoded).unwrap();
        assert_eq!(credentials, "test@example.com:token123");
    }
}
