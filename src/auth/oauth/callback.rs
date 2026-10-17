//! Loopback OAuth callback receiver (RFC 8252 §7.3).
//!
//! Receives one bounded HTTP request on 127.0.0.1, validates its callback
//! path and CSRF state, and returns an HTML result to the browser.

use anyhow::{Context, Result, bail};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

const ACCEPT_TIMEOUT: Duration = Duration::from_secs(300);
const REQUEST_LINE_LIMIT: u64 = 8192;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) struct CallbackResult {
    pub code: String,
}

/// Bind the loopback listener. Caller MUST bind before opening the browser so
/// the OS port is reserved.
pub(super) async fn bind(port: u16) -> Result<TcpListener> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind OAuth callback on {} (port in use?)", addr))
}

/// Wait for the OAuth provider to redirect to /callback and return the code.
/// Validates CSRF state. Timeout: 5 minutes.
pub(super) async fn receive(listener: TcpListener, expected_state: &str) -> Result<CallbackResult> {
    let (mut stream, _peer) = tokio::time::timeout(ACCEPT_TIMEOUT, listener.accept())
        .await
        .context("Timed out waiting for OAuth callback (5 min)")?
        .context("Failed to accept callback connection")?;

    let mut request_line = String::new();
    let mut reader = BufReader::new(&mut stream).take(REQUEST_LINE_LIMIT);
    tokio::time::timeout(REQUEST_TIMEOUT, reader.read_line(&mut request_line))
        .await
        .context("Timed out reading OAuth callback")?
        .context("Failed to read callback request")?;
    if !request_line.ends_with("\r\n") {
        bail!("OAuth callback request line is incomplete or too long");
    }

    let outcome = parse_callback(request_line.trim_end_matches("\r\n"), expected_state);

    let response_body = render_response(&outcome);
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response_body.len(),
        response_body
    );
    // Best-effort write; if it fails the browser still got the redirect.
    let _ = tokio::time::timeout(REQUEST_TIMEOUT, async {
        stream.write_all(response.as_bytes()).await?;
        stream.shutdown().await
    })
    .await;

    match outcome {
        Outcome::Ok { code } => Ok(CallbackResult { code }),
        Outcome::ProviderError { error, description } => bail!(
            "OAuth provider returned an error: {}{}",
            error,
            description.map(|d| format!(" — {}", d)).unwrap_or_default()
        ),
        Outcome::StateMismatch => bail!(
            "OAuth state mismatch — possible CSRF attempt. Try `atlassian-cli auth login` again."
        ),
        Outcome::MissingCode => bail!("OAuth callback missing `code` parameter"),
        Outcome::BadRequest(reason) => bail!("Malformed OAuth callback: {}", reason),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Ok {
        code: String,
    },
    ProviderError {
        error: String,
        description: Option<String>,
    },
    StateMismatch,
    MissingCode,
    BadRequest(String),
}

fn parse_callback(request_line: &str, expected_state: &str) -> Outcome {
    let mut parts = request_line.split(' ');
    let (Some("GET"), Some(target), Some("HTTP/1.0" | "HTTP/1.1"), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Outcome::BadRequest("expected a GET request line".into());
    };
    if target.split('?').next() != Some("/callback") {
        return Outcome::BadRequest("expected /callback".into());
    }
    let Ok(url) = reqwest::Url::parse(&format!("http://127.0.0.1{target}")) else {
        return Outcome::BadRequest("invalid callback URL".into());
    };
    if url.fragment().is_some() {
        return Outcome::BadRequest("callback target contains a fragment".into());
    }

    let mut code = None;
    let mut state = None;
    let mut provider_error = None;
    let mut error_description = None;
    for (key, value) in url.query_pairs() {
        let field = match key.as_ref() {
            "code" => &mut code,
            "state" => &mut state,
            "error" => &mut provider_error,
            "error_description" => &mut error_description,
            _ => continue,
        };
        if field.replace(value.into_owned()).is_some() {
            return Outcome::BadRequest(format!("duplicate {key} parameter"));
        }
    }

    let Some(state) = state else {
        return Outcome::BadRequest("missing state parameter".into());
    };
    if state != expected_state {
        return Outcome::StateMismatch;
    }
    if let Some(error) = provider_error {
        if code.is_some() || error.is_empty() {
            return Outcome::BadRequest("invalid authorization error response".into());
        }
        return Outcome::ProviderError {
            error,
            description: error_description,
        };
    }
    match code {
        Some(code) if !code.is_empty() => Outcome::Ok { code },
        _ => Outcome::MissingCode,
    }
}

fn render_response(outcome: &Outcome) -> String {
    let (title, body, color) = match outcome {
        Outcome::Ok { .. } => (
            "Logged in",
            "You can close this window and return to the terminal.",
            "#1f883d",
        ),
        Outcome::ProviderError { error, description } => (
            "Authorization failed",
            description.as_deref().unwrap_or(error),
            "#cf222e",
        ),
        Outcome::StateMismatch => (
            "Security check failed",
            "Possible CSRF — please run `atlassian-cli auth login` again.",
            "#cf222e",
        ),
        Outcome::MissingCode => (
            "Authorization incomplete",
            "The provider did not return an authorization code.",
            "#cf222e",
        ),
        Outcome::BadRequest(reason) => ("Bad request", reason.as_str(), "#cf222e"),
    };
    let body = body
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        r#"<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><title>atlassian-cli — {title}</title>
<style>body{{font-family:-apple-system,Segoe UI,sans-serif;display:flex;align-items:center;justify-content:center;min-height:100vh;margin:0;background:#f6f8fa;color:#1f2328}}
.card{{background:#fff;border:1px solid #d1d9e0;border-radius:8px;padding:48px 64px;text-align:center;max-width:480px}}
h1{{margin:0 0 12px;font-size:20px;color:{color}}}
p{{margin:0;color:#59636e}}</style></head>
<body><div class="card"><h1>{title}</h1><p>{body}</p></div></body></html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_happy_path() {
        let req = "GET /callback?code=ABC123&state=xyz HTTP/1.1";
        match parse_callback(req, "xyz") {
            Outcome::Ok { code } => assert_eq!(code, "ABC123"),
            o => panic!("expected Ok, got {:?}", o),
        }
    }

    #[test]
    fn rejects_state_mismatch() {
        let req = "GET /callback?code=ABC&state=wrong HTTP/1.1";
        assert_eq!(parse_callback(req, "expected"), Outcome::StateMismatch);
    }

    #[test]
    fn surfaces_provider_error() {
        let req =
            "GET /callback?error=access_denied&error_description=User%20cancelled&state=x HTTP/1.1";
        match parse_callback(req, "x") {
            Outcome::ProviderError { error, description } => {
                assert_eq!(error, "access_denied");
                assert_eq!(description.as_deref(), Some("User cancelled"));
            }
            o => panic!("expected ProviderError, got {:?}", o),
        }
    }

    #[test]
    fn provider_errors_require_the_expected_state() {
        let req = "GET /callback?error=invalid_scope HTTP/1.1";
        assert!(matches!(parse_callback(req, "x"), Outcome::BadRequest(_)));
        let req = "GET /callback?error=invalid_scope&state=wrong HTTP/1.1";
        assert_eq!(parse_callback(req, "x"), Outcome::StateMismatch);
    }

    #[test]
    fn rejects_non_get() {
        let req = "POST /callback?code=A&state=x HTTP/1.1";
        match parse_callback(req, "x") {
            Outcome::BadRequest(_) => {}
            o => panic!("expected BadRequest, got {:?}", o),
        }
    }

    #[test]
    fn missing_code() {
        let req = "GET /callback?state=x HTTP/1.1";
        assert_eq!(parse_callback(req, "x"), Outcome::MissingCode);
    }

    #[test]
    fn missing_state_is_bad_request() {
        let req = "GET /callback?code=A HTTP/1.1";
        match parse_callback(req, "x") {
            Outcome::BadRequest(_) => {}
            o => panic!("expected BadRequest, got {:?}", o),
        }
    }

    #[test]
    fn percent_decodes_code() {
        let req = "GET /callback?code=foo%2Fbar&state=x HTTP/1.1";
        match parse_callback(req, "x") {
            Outcome::Ok { code } => assert_eq!(code, "foo/bar"),
            o => panic!("expected Ok, got {:?}", o),
        }
    }

    #[test]
    fn rejects_ambiguous_or_misdirected_callbacks() {
        for target in [
            "/elsewhere?code=A&state=x",
            "/callback?code=A&state=x&state=x",
            "/callback?code=A&code=B&state=x",
            "/callback?error=denied&code=A&state=x",
            "/callback?code=A&state=x#fragment",
        ] {
            assert!(
                matches!(
                    parse_callback(&format!("GET {target} HTTP/1.1"), "x"),
                    Outcome::BadRequest(_)
                ),
                "{target}"
            );
        }
        assert_eq!(
            parse_callback("GET /callback?code=&state=x HTTP/1.1", "x"),
            Outcome::MissingCode
        );
    }

    #[test]
    fn decodes_form_query_values() {
        let req = "GET /callback?code=a%2Bb+c&st%61te=x HTTP/1.1";
        assert_eq!(
            parse_callback(req, "x"),
            Outcome::Ok {
                code: "a+b c".into()
            }
        );
    }

    #[test]
    fn browser_response_escapes_provider_text() {
        let html = render_response(&Outcome::ProviderError {
            error: "denied".into(),
            description: Some("<script>alert(1)</script>&".into()),
        });
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;&amp;"));
    }

    #[tokio::test]
    async fn end_to_end_loopback_delivers_code() {
        // Real loopback handshake on an ephemeral port.
        let listener = bind(0).await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move { receive(listener, "state-abc").await });

        // Pretend to be the browser
        let client = tokio::spawn(async move {
            let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
            s.write_all(b"GET /callback?code=success-").await.unwrap();
            let mut early_response = [0u8; 1];
            assert!(
                tokio::time::timeout(Duration::from_millis(20), s.read(&mut early_response))
                    .await
                    .is_err()
            );
            s.write_all(b"code&state=state-abc HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
                .await
                .unwrap();
            let mut buf = String::new();
            tokio::io::AsyncReadExt::read_to_string(&mut s, &mut buf)
                .await
                .ok();
            buf
        });

        let result = server.await.unwrap().unwrap();
        assert_eq!(result.code, "success-code");
        let response = client.await.unwrap();
        assert!(response.contains("Logged in"));
    }
}
