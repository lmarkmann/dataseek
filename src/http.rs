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

use std::time::Duration;

use serde_json::Value;

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
}

impl SourceError {
    /// Failures that say the host is down rather than this query being bad.
    /// The search loop skips such a source for a few minutes afterwards.
    pub fn is_outage(&self) -> bool {
        match self {
            Self::Unreachable(_) | Self::Timeout | Self::Blocked => true,
            Self::Status(code) => *code >= 500,
            Self::RateLimited | Self::Unauthorized(_) | Self::Shape(_) => {
                false
            }
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
    use super::*;

    #[test]
    fn only_host_failures_count_as_outages() {
        assert!(SourceError::Timeout.is_outage());
        assert!(SourceError::Status(503).is_outage());
        assert!(!SourceError::Status(404).is_outage());
        assert!(!SourceError::Unauthorized(401).is_outage());
        assert!(!SourceError::RateLimited.is_outage());
    }

    #[test]
    fn cloudflare_interstitials_are_recognized() {
        assert!(looks_like_challenge(
            "<html><title>Just a moment...</title></html>"
        ));
        assert!(!looks_like_challenge("{\"message\":\"Unauthorized\"}"));
    }
}
