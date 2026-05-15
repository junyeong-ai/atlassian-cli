//! Loopback OAuth callback receiver (RFC 8252 §7.3).
//!
//! Binds 127.0.0.1:{port}, accepts a single connection, parses the redirect URI
//! query for `code` + `state`, validates CSRF state, returns an HTML page, and
//! shuts down. No third-party HTTP framework — direct tokio TCP, ~80 lines.

use anyhow::{Context, Result, bail};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const ACCEPT_TIMEOUT: Duration = Duration::from_secs(300);
const READ_BUFFER: usize = 8192;

#[derive(Debug)]
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

    // Read up to READ_BUFFER bytes — request line + headers fit easily.
    let mut buf = vec![0u8; READ_BUFFER];
    let n = stream
        .read(&mut buf)
        .await
        .context("Failed to read callback request")?;
    let request = String::from_utf8_lossy(&buf[..n]);

    let request_line = request
        .lines()
        .next()
        .context("Empty HTTP request from OAuth callback")?;

    let outcome = parse_callback(request_line, expected_state);

    let response_body = render_response(&outcome);
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response_body.len(),
        response_body
    );
    // Best-effort write; if it fails the browser still got the redirect.
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;

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
    // Expected: "GET /callback?code=...&state=... HTTP/1.1"
    let mut parts = request_line.split_whitespace();
    let method = parts.next();
    let target = parts.next();

    let Some(target) = target else {
        return Outcome::BadRequest("malformed request line".into());
    };
    if method != Some("GET") {
        return Outcome::BadRequest(format!("expected GET, got {:?}", method));
    }

    let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");

    let mut code = None;
    let mut state = None;
    let mut provider_error = None;
    let mut error_description = None;

    for pair in query.split('&') {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        let decoded = url_decode(v);
        match k {
            "code" => code = Some(decoded),
            "state" => state = Some(decoded),
            "error" => provider_error = Some(decoded),
            "error_description" => error_description = Some(decoded),
            _ => {}
        }
    }

    if let Some(err) = provider_error {
        return Outcome::ProviderError {
            error: err,
            description: error_description,
        };
    }

    let Some(state) = state else {
        return Outcome::BadRequest("missing `state` parameter".into());
    };
    if state != expected_state {
        return Outcome::StateMismatch;
    }

    match code {
        Some(c) => Outcome::Ok { code: c },
        None => Outcome::MissingCode,
    }
}

/// Minimal percent-decoding for OAuth query values.
/// `+` → space, `%XX` → byte. Tolerant: malformed escapes pass through.
fn url_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                match (hi, lo) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 3;
                    }
                    _ => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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
    fn provider_error_skips_state_check() {
        // If the provider returns an error, we surface it even without state.
        let req = "GET /callback?error=invalid_scope HTTP/1.1";
        match parse_callback(req, "x") {
            Outcome::ProviderError { error, .. } => assert_eq!(error, "invalid_scope"),
            o => panic!("expected ProviderError, got {:?}", o),
        }
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
    fn url_decode_handles_plus_as_space() {
        assert_eq!(url_decode("hello+world"), "hello world");
    }

    #[test]
    fn url_decode_tolerates_bad_escape() {
        // Malformed %ZZ should pass through (not panic)
        assert_eq!(url_decode("a%ZZb"), "a%ZZb");
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
            s.write_all(
                b"GET /callback?code=success-code&state=state-abc HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            )
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
