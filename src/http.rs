//! The one blocking HTTP client every source shares, and the failure taxonomy
//! the search loop acts on.
//!
//! Every request identifies dataseek, its repository and its contact
//! address in the User-Agent, which is what DataCite, NCBI and other polite
//! pools key their better rate tier on. Non-2xx statuses come back as
//! values, not errors, so [`SourceError`] can say whether a failure is worth
//! remembering (a dead or throttling host) or only this query's problem (a
//! rejected key).
//! A single 429 is retried once when the server asks for a short wait;
//! longer waits are reported instead of slept through, and a failed connect
//! is retried once. Bodies are decoded leniently: a stray invalid byte in a
//! 30 MB catalog should cost one character, not the source.

use std::ffi::OsStr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use serde_json::Value;

static OFFLINE: AtomicBool = AtomicBool::new(false);
/// `--connect-timeout` unless the user sets it. A host that misses a limit the
/// user chose is not down, only slower than they asked for.
pub const DEFAULT_CONNECT_SECS: u64 = 10;
static CONNECT_SECS: AtomicU64 = AtomicU64::new(DEFAULT_CONNECT_SECS);

/// From now on every request fails at once with [`SourceError::Offline`],
/// which the search loop answers from the cache and never records as an
/// outage. Set once, from `main`, for `--offline`.
pub fn go_offline() {
    OFFLINE.store(true, Ordering::Relaxed);
}

pub fn is_offline() -> bool {
    OFFLINE.load(Ordering::Relaxed)
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
    /// Only rustls builds tell a refused certificate apart; native-tls
    /// reports every handshake failure as one opaque error.
    #[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
    #[error("could not verify the certificate ({0})")]
    Certificate(String),
    #[error("timed out")]
    Timeout,
    /// The search deadline passed while this source still had pages to ask.
    /// The rows in hand belong to a partial answer, so nothing is cached
    /// from it and no host is marked down.
    #[error("stopped by the search deadline")]
    Stopped,
    #[error("did not connect within --connect-timeout ({0} s)")]
    ConnectLimit(u64),
    #[error("rate limited by the source")]
    RateLimited,
    #[error("rejected the credentials (HTTP {0})")]
    Unauthorized(u16),
    /// A 401 or 403 to a request that carried no key: a firewall rule or an
    /// access policy, often for this query alone.
    #[error("refused the request (HTTP {0})")]
    Refused(u16),
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
    /// Failures that say the host is down, or wants no more requests for now,
    /// rather than this query being bad. The search loop skips such a source
    /// for a few minutes afterwards, so a throttled host is not asked again
    /// on every search (OSF's `/trove/` answered 17 requests in 3 minutes
    /// with an hour of 429s, October 2026).
    pub fn is_outage(&self) -> bool {
        match self {
            Self::Unreachable(_)
            | Self::Timeout
            | Self::Blocked
            | Self::RateLimited => true,
            Self::Status(code) => *code >= 500,
            Self::Stopped
            | Self::Certificate(_)
            | Self::Unauthorized(_)
            | Self::Refused(_)
            | Self::Shape(_)
            | Self::ConnectLimit(_)
            | Self::Offline => false,
        }
    }

    pub fn shape(what: impl Into<String>) -> Self {
        Self::Shape(terminal_safe(what.into()))
    }
}

/// Error text that came from upstream, with its control characters
/// replaced: a provider that puts an escape sequence into a tag name or a
/// header must not get to drive the terminal the error is printed on.
fn terminal_safe(upstream: impl std::fmt::Display) -> String {
    crate::record::printable(&upstream.to_string()).collect()
}

pub struct Http {
    agent: ureq::Agent,
}

impl Http {
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .user_agent(format!(
                "dataseek/{} (+{}; mailto:{CONTACT})",
                env!("CARGO_PKG_VERSION"),
                env!("CARGO_PKG_REPOSITORY")
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

/// The OS's trust store everywhere, so a root certificate installed for a
/// TLS-inspecting proxy is trusted: through the OS's own TLS stack on Windows
/// and macOS, through rustls and rustls-platform-verifier elsewhere.
/// `Cargo.toml` enables the matching ureq features per platform.
fn tls() -> ureq::tls::TlsConfig {
    let builder = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier);
    #[cfg(any(windows, target_os = "macos"))]
    let builder = builder.provider(ureq::tls::TlsProvider::NativeTls);
    builder.build()
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
    /// Whether a key went with the request, which is what lets a 401 or 403
    /// mean the key was rejected.
    keyed: bool,
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
            keyed: false,
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

    /// A header that carries a key.
    pub fn key_header(mut self, key: &str, value: impl ToString) -> Self {
        self.keyed = true;
        self.header(key, value)
    }

    /// A query parameter that carries a key, for the APIs that take it
    /// nowhere else; the URL is never printed (ADR 0009).
    pub fn key_query(mut self, key: &str, value: impl ToString) -> Self {
        self.keyed = true;
        self.query(key, value)
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
        if status == 429 || (status == 403 && quota_spent(response.headers()))
        {
            let wait = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok())
                .map(Duration::from_secs);
            return Err(match (status, wait) {
                (_, Some(wait)) => Retry::After(wait),
                (429, None) => Retry::After(Duration::from_secs(1)),
                _ => Retry::Fail(SourceError::RateLimited),
            });
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
            401 | 403 if self.keyed => {
                Err(Retry::Fail(SourceError::Unauthorized(status)))
            }
            401 | 403 => Err(Retry::Fail(SourceError::Refused(status))),
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
    let limit = CONNECT_SECS.load(Ordering::Relaxed);
    match error {
        ureq::Error::Timeout(ureq::Timeout::Connect)
            if limit != DEFAULT_CONNECT_SECS =>
        {
            SourceError::ConnectLimit(limit)
        }
        ureq::Error::Timeout(_) => SourceError::Timeout,
        #[cfg(not(any(windows, target_os = "macos")))]
        ureq::Error::Rustls(_) => {
            SourceError::Certificate(terminal_safe(error))
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        ureq::Error::Io(io) if refused_certificate(io) => {
            SourceError::Certificate(terminal_safe(error))
        }
        other => SourceError::Unreachable(terminal_safe(other)),
    }
}

/// rustls reports a certificate it could not verify inside an I/O error
/// from the handshake. The trust store failing to load arrives as
/// `ureq::Error::Rustls` instead, before any connection.
#[cfg(not(any(windows, target_os = "macos")))]
fn refused_certificate(error: &std::io::Error) -> bool {
    error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
        .is_some_and(|e| matches!(e, rustls::Error::InvalidCertificate(_)))
}

/// What to do when no certificate could be verified. On Linux,
/// `SSL_CERT_FILE` or `SSL_CERT_DIR` replaces the whole system store, so one
/// naming a path that cannot be read leaves nothing to trust; otherwise the
/// system has no store, or not the root the server needs.
pub fn certificate_hint() -> String {
    let file = std::env::var_os("SSL_CERT_FILE");
    let dirs = std::env::var_os("SSL_CERT_DIR");
    certificate_hint_for(file.as_deref(), dirs.as_deref())
}

fn certificate_hint_for(file: Option<&OsStr>, dirs: Option<&OsStr>) -> String {
    let unreadable_file = file.map(Path::new).filter(|path| {
        !std::fs::File::open(path)
            .and_then(|f| f.metadata())
            .is_ok_and(|m| m.is_file())
    });
    if let Some(path) = unreadable_file {
        return format!(
            "SSL_CERT_FILE names \"{}\", which cannot be read; point it at a PEM bundle of root certificates, or unset it to use the system's",
            path.display()
        );
    }
    let unreadable_dir = dirs.and_then(|dirs| {
        std::env::split_paths(dirs).find(|dir| {
            !dir.as_os_str().is_empty() && std::fs::read_dir(dir).is_err()
        })
    });
    if let Some(dir) = unreadable_dir {
        return format!(
            "SSL_CERT_DIR names \"{}\", which cannot be read; point it at a directory of root certificates, or unset it to use the system's",
            dir.display()
        );
    }
    "install the distribution's ca-certificates package, or point SSL_CERT_FILE at a PEM bundle of root certificates".to_owned()
}

/// GitHub and other hosts answer an exhausted quota with 403 rather than
/// 429, marked by a spent `x-ratelimit-remaining` or a `retry-after`.
fn quota_spent(headers: &ureq::http::HeaderMap) -> bool {
    headers.get("x-ratelimit-remaining").is_some_and(|v| v.as_bytes() == b"0")
        || headers.contains_key("retry-after")
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
            (SourceError::ConnectLimit(2), false),
            (SourceError::Certificate("unknown issuer".into()), false),
            (SourceError::Blocked, true),
            (SourceError::Status(503), true),
            (SourceError::Status(500), true),
            (SourceError::Status(404), false),
            (SourceError::Unauthorized(401), false),
            (SourceError::Refused(403), false),
            (SourceError::RateLimited, true),
            (SourceError::Stopped, false),
            (SourceError::shape("no hits"), false),
        ] {
            assert_eq!(error.is_outage(), outage, "{error:?}");
        }
    }

    #[test]
    fn a_connect_timeout_the_user_chose_is_not_an_outage() {
        let connect = ureq::Error::Timeout(ureq::Timeout::Connect);
        assert!(transport(&connect).is_outage(), "the default limit missed");

        connect_within(Duration::from_secs(2));
        let error = transport(&connect);
        assert!(!error.is_outage(), "{error:?}");
        assert!(error.to_string().contains("--connect-timeout"), "{error}");

        let slow_answer = ureq::Error::Timeout(ureq::Timeout::Global);
        assert!(transport(&slow_answer).is_outage(), "a hung host is down");
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn a_refused_certificate_is_its_own_failure() {
        let unknown_issuer = rustls::Error::InvalidCertificate(
            rustls::CertificateError::UnknownIssuer,
        );
        let handshake = ureq::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            unknown_issuer,
        ));
        assert!(
            matches!(transport(&handshake), SourceError::Certificate(_)),
            "{handshake}"
        );
        let no_store = ureq::Error::Rustls(rustls::Error::General(
            "No CA certificates were loaded from the system".into(),
        ));
        assert!(
            matches!(transport(&no_store), SourceError::Certificate(_)),
            "{no_store}"
        );
        let reset =
            ureq::Error::Io(std::io::ErrorKind::ConnectionReset.into());
        assert!(
            matches!(transport(&reset), SourceError::Unreachable(_)),
            "{reset}"
        );
    }

    #[test]
    fn the_certificate_hint_names_a_variable_that_points_nowhere() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("roots.pem");
        std::fs::write(&bundle, "").unwrap();
        let missing = dir.path().join("missing.pem");

        let system = certificate_hint_for(None, None);
        assert!(system.contains("ca-certificates"), "{system}");
        assert_eq!(
            certificate_hint_for(Some(bundle.as_os_str()), None),
            system,
            "a readable bundle is not the variable's fault"
        );

        let hint = certificate_hint_for(Some(missing.as_os_str()), None);
        assert!(hint.starts_with("SSL_CERT_FILE names"), "{hint}");
        assert!(hint.contains(&*missing.to_string_lossy()), "{hint}");
        let hint = certificate_hint_for(Some(dir.path().as_os_str()), None);
        assert!(hint.starts_with("SSL_CERT_FILE names"), "{hint}");

        let dirs = std::env::join_paths([dir.path(), &missing]).unwrap();
        let hint = certificate_hint_for(None, Some(&dirs));
        assert!(hint.starts_with("SSL_CERT_DIR names"), "{hint}");
        assert!(hint.contains(&*missing.to_string_lossy()), "{hint}");
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
    fn a_403_with_a_spent_quota_is_rate_limited_and_any_other_403_is_not() {
        for (headers, expected_limited) in [
            (vec!["X-RateLimit-Remaining: 0"], true),
            (vec!["Retry-After: 60"], true),
            (vec!["X-RateLimit-Remaining: 12"], false),
            (vec![], false),
        ] {
            let (url, server) =
                serve(vec![response("403 Forbidden", &headers, b"")]);
            let outcome = Http::new()
                .get(&url)
                .key_header("Authorization", "Bearer token")
                .text();
            assert_eq!(
                matches!(outcome, Err(SourceError::RateLimited)),
                expected_limited,
                "{headers:?}: {outcome:?}"
            );
            if !expected_limited {
                assert!(
                    matches!(outcome, Err(SourceError::Unauthorized(403))),
                    "{headers:?}: {outcome:?}"
                );
            }
            server.join().unwrap();
        }
    }

    #[test]
    fn statuses_map_to_the_failure_taxonomy() {
        let cloudflare = b"<html><title>Just a moment...</title></html>";
        let waf = b"<title>Request Blocked by WAF</title>";
        for (status, body, keyed, expected) in [
            ("403 Forbidden", &cloudflare[..], false, SourceError::Blocked),
            (
                "403 Forbidden",
                b"<div id=\"cf-chl-widget\">",
                true,
                SourceError::Blocked,
            ),
            (
                "401 Unauthorized",
                b"{\"message\":\"bad key\"}",
                true,
                SourceError::Unauthorized(401),
            ),
            ("403 Forbidden", b"denied", true, SourceError::Unauthorized(403)),
            ("403 Forbidden", &waf[..], false, SourceError::Refused(403)),
            ("401 Unauthorized", b"", false, SourceError::Refused(401)),
            ("503 Service Unavailable", b"", false, SourceError::Status(503)),
            ("404 Not Found", b"", false, SourceError::Status(404)),
        ] {
            let (url, server) = serve(vec![response(status, &[], body)]);
            let http = Http::new();
            let call = http.get(&url);
            let call =
                if keyed { call.key_query("api_key", "k") } else { call };
            let outcome = call.text().unwrap_err();
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
                "user-agent: dataseek/{} (+{}; mailto:{CONTACT})",
                env!("CARGO_PKG_VERSION"),
                env!("CARGO_PKG_REPOSITORY")
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
