//! The one blocking HTTP client every source shares, and the failure taxonomy
//! the search loop acts on.
//!
//! Every request identifies dataseek and its contact address in the
//! User-Agent, which is what DataCite, NCBI and other polite pools key their
//! better rate tier on. Non-2xx statuses come back as values, not errors, so
//! [`SourceError`] can say whether a failure is worth remembering (a dead
//! host) or only this query's problem (a rejected key). A single 429 is
//! retried once when the server asks for a short wait; longer waits are
//! reported instead of slept through, and a failed connect is retried once.
//! Bodies are decoded leniently: a stray invalid byte in a 30 MB catalog
//! should cost one character, not the source.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use serde_json::Value;

static OFFLINE: AtomicBool = AtomicBool::new(false);
static CONNECT_SECS: AtomicU64 = AtomicU64::new(10);

/// From now on every request fails at once with [`SourceError::Offline`],
/// which the search loop answers from the cache and never records as an
/// outage. Set once, from `main`, for `--offline`.
pub fn go_offline() {
    OFFLINE.store(true, Ordering::Relaxed);
}

/// How long a host gets to accept the connection, for clients built after
/// this; `--connect-timeout`.
pub fn connect_within(limit: Duration) {
    CONNECT_SECS.store(limit.as_secs(), Ordering::Relaxed);
}

/// Contact address sent to every source. It is the project's, never a
/// user's: whoever runs dataseek, the remote logs see this one.
pub const CONTACT: &str = "user@dataseek.dev";

const BODY_LIMIT: u64 = 64 * 1024 * 1024;
const LONGEST_RETRY_WAIT: Duration = Duration::from_secs(4);
/// DNS and connect failures under a burst of parallel lookups are often
/// momentary; one retry after this pause separates those from a dead host.
const CONNECT_RETRY_PAUSE: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, thiserror::Error)]
pub enum SourceError {
    #[error("unreachable ({0})")]
    Unreachable(String),
    #[error("timed out")]
    Timeout,
    #[error("rate limited by the source")]
    RateLimited,
    #[error("rejected the credentials (HTTP {0})")]
    Unauthorized(u16),
    #[error("answered HTTP {0}")]
    Status(u16),
    #[error("sent a challenge page instead of results")]
    Blocked,
    #[error("returned an unexpected shape: {0}")]
    Shape(String),
    #[error("not asked (--offline)")]
    Offline,
}

impl SourceError {
    /// Failures that say the host is down rather than this query being bad.
    /// The search loop skips such a source for a few minutes afterwards.
    pub fn is_outage(&self) -> bool {
        match self {
            Self::Unreachable(_) | Self::Timeout | Self::Blocked => true,
            Self::Status(code) => *code >= 500,
            Self::RateLimited
            | Self::Unauthorized(_)
            | Self::Shape(_)
            | Self::Offline => false,
        }
    }

    pub fn shape(what: impl Into<String>) -> Self {
        Self::Shape(what.into())
    }
}

pub struct Http {
    agent: ureq::Agent,
}

impl Http {
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .user_agent(format!(
                "dataseek/{} (mailto:{CONTACT})",
                env!("CARGO_PKG_VERSION")
            ))
            .timeout_global(Some(Duration::from_secs(20)))
            .timeout_connect(Some(Duration::from_secs(
                CONNECT_SECS.load(Ordering::Relaxed),
            )))
            .tls_config(tls())
            .build()
            .into();
        Self { agent }
    }

    pub fn get(&self, url: &str) -> Call<'_> {
        Call::new(self, Method::Get, url)
    }

    pub fn post(&self, url: &str) -> Call<'_> {
        Call::new(self, Method::Post, url)
    }
}

/// The OS's TLS stack and trust store on Windows and macOS, rustls with the
/// bundled Mozilla roots elsewhere; `Cargo.toml` enables the matching ureq
/// feature per platform.
#[cfg(any(windows, target_os = "macos"))]
fn tls() -> ureq::tls::TlsConfig {
    ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::NativeTls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build()
}

#[cfg(not(any(windows, target_os = "macos")))]
fn tls() -> ureq::tls::TlsConfig {
    ureq::tls::TlsConfig::default()
}

#[derive(Clone, Copy)]
enum Method {
    Get,
    Post,
}

/// One request being assembled. Query values are percent-encoded by ureq.
pub struct Call<'a> {
    http: &'a Http,
    method: Method,
    url: String,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    body: Option<Value>,
    timeout: Duration,
}

impl<'a> Call<'a> {
    fn new(http: &'a Http, method: Method, url: &str) -> Self {
        Self {
            http,
            method,
            url: url.to_owned(),
            query: Vec::new(),
            headers: Vec::new(),
            body: None,
            timeout: Duration::from_secs(15),
        }
    }

    #[expect(
        clippy::needless_pass_by_value,
        reason = "callers pass literals and numbers; borrowing each adds noise"
    )]
    pub fn query(mut self, key: &str, value: impl ToString) -> Self {
        self.query.push((key.to_owned(), value.to_string()));
        self
    }

    #[expect(
        clippy::needless_pass_by_value,
        reason = "callers pass literals and numbers; borrowing each adds noise"
    )]
    pub fn header(mut self, key: &str, value: impl ToString) -> Self {
        self.headers.push((key.to_owned(), value.to_string()));
        self
    }

    pub fn json_body(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }

    /// Whole-catalog downloads are megabytes; they get a longer deadline.
    pub fn slow(mut self) -> Self {
        self.timeout = Duration::from_secs(90);
        self
    }

    pub fn json(self) -> Result<Value, SourceError> {
        let text = self.header("Accept", "application/json").text()?;
        serde_json::from_str(&text)
            .map_err(|e| SourceError::shape(format!("not JSON: {e}")))
    }

    pub fn text(self) -> Result<String, SourceError> {
        match self.send() {
            Err(Retry::After(wait)) if wait <= LONGEST_RETRY_WAIT => {
                std::thread::sleep(wait);
                self.send().map_err(Retry::into_error)
            }
            Err(Retry::Fail(SourceError::Unreachable(_))) => {
                std::thread::sleep(CONNECT_RETRY_PAUSE);
                self.send().map_err(Retry::into_error)
            }
            other => other.map_err(Retry::into_error),
        }
    }

    fn send(&self) -> Result<String, Retry> {
        if OFFLINE.load(Ordering::Relaxed) {
            return Err(Retry::Fail(SourceError::Offline));
        }
        let result = match self.method {
            Method::Get => {
                let mut request = self.http.agent.get(&self.url);
                for (k, v) in &self.query {
                    request = request.query(k, v);
                }
                for (k, v) in &self.headers {
                    request = request.header(k, v);
                }
                request
                    .config()
                    .timeout_global(Some(self.timeout))
                    .build()
                    .call()
            }
            Method::Post => {
                let mut request = self.http.agent.post(&self.url);
                for (k, v) in &self.query {
                    request = request.query(k, v);
                }
                for (k, v) in &self.headers {
                    request = request.header(k, v);
                }
                let request = request
                    .config()
                    .timeout_global(Some(self.timeout))
                    .build();
                match &self.body {
                    Some(body) => request.send_json(body),
                    None => request.send_empty(),
                }
            }
        };
        let mut response = result.map_err(|e| Retry::Fail(transport(&e)))?;
        let status = response.status().as_u16();
        if status == 429 {
            let wait = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok())
                .map_or(Duration::from_secs(1), Duration::from_secs);
            return Err(Retry::After(wait));
        }
        let body = response
            .body_mut()
            .with_config()
            .limit(BODY_LIMIT)
            .read_to_vec()
            .map_err(|e| Retry::Fail(transport(&e)))?;
        let body = String::from_utf8(body).unwrap_or_else(|invalid| {
            String::from_utf8_lossy(invalid.as_bytes()).into_owned()
        });
        match status {
            200..=299 => Ok(body),
            401 | 403 if looks_like_challenge(&body) => {
                Err(Retry::Fail(SourceError::Blocked))
            }
            401 | 403 => Err(Retry::Fail(SourceError::Unauthorized(status))),
            _ => Err(Retry::Fail(SourceError::Status(status))),
        }
    }
}

enum Retry {
    After(Duration),
    Fail(SourceError),
}

impl Retry {
    fn into_error(self) -> SourceError {
        match self {
            Self::After(_) => SourceError::RateLimited,
            Self::Fail(e) => e,
        }
    }
}

fn transport(error: &ureq::Error) -> SourceError {
    match error {
        ureq::Error::Timeout(_) => SourceError::Timeout,
        other => SourceError::Unreachable(other.to_string()),
    }
}

/// Cloudflare and similar bot walls answer 403 with an HTML interstitial.
fn looks_like_challenge(body: &str) -> bool {
    body.contains("Just a moment...") || body.contains("cf-chl")
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::thread::JoinHandle;
    use std::time::Instant;

    use super::*;

    #[test]
    fn only_host_failures_count_as_outages() {
        for (error, outage) in [
            (SourceError::Unreachable("dns".into()), true),
            (SourceError::Timeout, true),
            (SourceError::Blocked, true),
            (SourceError::Status(503), true),
            (SourceError::Status(500), true),
            (SourceError::Status(404), false),
            (SourceError::Unauthorized(401), false),
            (SourceError::RateLimited, false),
            (SourceError::shape("no hits"), false),
        ] {
            assert_eq!(error.is_outage(), outage, "{error:?}");
        }
    }

    /// A local server that answers each connection with the next canned
    /// response and hands back the request heads it saw.
    fn serve(responses: Vec<Vec<u8>>) -> (String, JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/search", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            responses
                .into_iter()
                .map(|response| {
                    let (mut stream, _) = listener.accept().unwrap();
                    let mut head = String::new();
                    let mut reader = BufReader::new(&stream);
                    loop {
                        let mut line = String::new();
                        reader.read_line(&mut line).unwrap();
                        if line.trim().is_empty() {
                            break;
                        }
                        head.push_str(&line);
                    }
                    stream.write_all(&response).unwrap();
                    head
                })
                .collect()
        });
        (url, handle)
    }

    fn response(status: &str, headers: &[&str], body: &[u8]) -> Vec<u8> {
        let mut out = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        for header in headers {
            out.push_str(header);
            out.push_str("\r\n");
        }
        out.push_str("\r\n");
        let mut bytes = out.into_bytes();
        bytes.extend_from_slice(body);
        bytes
    }

    #[test]
    fn a_short_retry_after_is_waited_out_once() {
        let (url, server) = serve(vec![
            response("429 Too Many Requests", &["Retry-After: 0"], b""),
            response("200 OK", &[], b"results"),
        ]);
        assert_eq!(Http::new().get(&url).text().unwrap(), "results");
        assert_eq!(server.join().unwrap().len(), 2);
    }

    #[test]
    fn a_long_retry_after_is_reported_instead_of_slept() {
        let (url, server) = serve(vec![response(
            "429 Too Many Requests",
            &["Retry-After: 60"],
            b"",
        )]);
        let started = Instant::now();
        let outcome = Http::new().get(&url).text();
        assert!(
            matches!(outcome, Err(SourceError::RateLimited)),
            "{outcome:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(server.join().unwrap().len(), 1);
    }

    #[test]
    fn statuses_map_to_the_failure_taxonomy() {
        let cloudflare = b"<html><title>Just a moment...</title></html>";
        for (status, body, expected) in [
            ("403 Forbidden", &cloudflare[..], SourceError::Blocked),
            (
                "403 Forbidden",
                b"<div id=\"cf-chl-widget\">",
                SourceError::Blocked,
            ),
            (
                "401 Unauthorized",
                b"{\"message\":\"bad key\"}",
                SourceError::Unauthorized(401),
            ),
            ("403 Forbidden", b"denied", SourceError::Unauthorized(403)),
            ("503 Service Unavailable", b"", SourceError::Status(503)),
            ("404 Not Found", b"", SourceError::Status(404)),
        ] {
            let (url, server) = serve(vec![response(status, &[], body)]);
            let outcome = Http::new().get(&url).text().unwrap_err();
            assert_eq!(outcome.to_string(), expected.to_string(), "{status}");
            server.join().unwrap();
        }
    }

    #[test]
    fn an_invalid_byte_costs_one_character_not_the_body() {
        let (url, server) =
            serve(vec![response("200 OK", &[], b"sea\xff ice")]);
        assert_eq!(Http::new().get(&url).text().unwrap(), "sea\u{fffd} ice");
        server.join().unwrap();
    }

    #[test]
    fn a_page_that_is_not_json_is_a_shape_change() {
        let (url, server) =
            serve(vec![response("200 OK", &[], b"<html>maintenance</html>")]);
        let outcome = Http::new().get(&url).json();
        assert!(matches!(outcome, Err(SourceError::Shape(_))), "{outcome:?}");
        server.join().unwrap();
    }

    #[test]
    fn requests_name_dataseek_and_its_contact_and_carry_the_query() {
        let (url, server) = serve(vec![response("200 OK", &[], b"{}")]);
        Http::new().get(&url).query("q", "sea ice").json().unwrap();
        let head = server.join().unwrap().remove(0).to_lowercase();
        assert!(head.starts_with("get /search?q=sea"), "{head}");
        assert!(
            head.contains(&format!(
                "user-agent: dataseek/{} (mailto:{CONTACT})",
                env!("CARGO_PKG_VERSION")
            )),
            "{head}"
        );
        assert!(head.contains("accept: application/json"), "{head}");
    }

    #[test]
    fn a_closed_port_is_unreachable() {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let outcome =
            Http::new().get(&format!("http://127.0.0.1:{port}/")).text();
        assert!(
            matches!(outcome, Err(SourceError::Unreachable(_))),
            "{outcome:?}"
        );
    }
}
