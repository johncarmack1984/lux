//! The transport seam.
//!
//! Every request this crate makes goes through [`Http`], which exists for one
//! reason: a live-service integration whose failure modes only appear against
//! the real API is an integration nobody can trust on a Sunday morning. With
//! the seam, the whole client — pagination, rate-limit headers, a 429, an
//! expired token, a Live vertex with nothing live — runs against recorded
//! responses in `cargo test`.
//!
//! [`ReqwestHttp`] is the one that talks to the internet. It is deliberately
//! thin: it maps a [`HttpRequest`] onto reqwest and hands back status, headers
//! and body untouched, so the parts worth testing stay on the testable side of
//! the seam.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use crate::error::Error;

/// A boxed future, because the trait is used behind generics in an async
/// Lambda: `async fn` in a trait can't promise `Send`, and the runtime needs
/// it to.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The two verbs this crate knows. There is no `PUT`, `PATCH` or `DELETE`
/// here, and [`Method::Post`] exists only for the OAuth token endpoint — see
/// the crate docs on why that boundary is structural rather than a promise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
        }
    }
}

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: Method::Get,
            url: url.into(),
            headers: vec![("accept".into(), "application/json".into())],
            body: None,
        }
    }

    /// A form-encoded `POST` — the OAuth token endpoint's only shape.
    pub fn form(url: impl Into<String>, body: String) -> Self {
        Self {
            method: Method::Post,
            url: url.into(),
            headers: vec![
                ("accept".into(), "application/json".into()),
                (
                    "content-type".into(),
                    "application/x-www-form-urlencoded".into(),
                ),
            ],
            body: Some(body),
        }
    }

    pub fn with_bearer(mut self, access_token: &str) -> Self {
        self.headers
            .push(("authorization".into(), format!("Bearer {access_token}")));
        self
    }
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl HttpResponse {
    pub fn new(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Case-insensitive header lookup — HTTP/2 lowercases, HTTP/1.1 doesn't,
    /// and Planning Center's rate-limit headers are documented in mixed case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

pub trait Http: Send + Sync {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, Error>>;
}

/// The real transport.
///
/// Built on the workspace's `rustls-no-provider` reqwest, so **the process
/// must install a crypto provider before the first request** — the same
/// requirement `lux-auth` carries, and the same one-liner the Lambdas already
/// run in `main`.
pub struct ReqwestHttp {
    client: reqwest::Client,
}

impl ReqwestHttp {
    /// A client whose timeout is shorter than the poll interval, so a stalled
    /// request fails in time for the next tick instead of queueing behind it.
    pub fn new() -> Result<Self, Error> {
        Self::with_timeout(Duration::from_secs(5))
    }

    pub fn with_timeout(timeout: Duration) -> Result<Self, Error> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| Error::Transport(e.to_string()))?;
        Ok(Self { client })
    }
}

impl Http for ReqwestHttp {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, Error>> {
        Box::pin(async move {
            let mut builder = match request.method {
                Method::Get => self.client.get(&request.url),
                Method::Post => self.client.post(&request.url),
            };
            for (name, value) in &request.headers {
                builder = builder.header(name, value);
            }
            if let Some(body) = request.body {
                builder = builder.body(body);
            }
            let response = builder
                .send()
                .await
                .map_err(|e| Error::Transport(e.to_string()))?;
            let status = response.status().as_u16();
            let headers = response
                .headers()
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .to_str()
                        .ok()
                        .map(|v| (name.as_str().to_owned(), v.to_owned()))
                })
                .collect();
            let body = response
                .text()
                .await
                .map_err(|e| Error::Transport(e.to_string()))?;
            Ok(HttpResponse {
                status,
                headers,
                body,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_lookup_ignores_case() {
        let response = HttpResponse::new(200, "{}")
            .with_header("X-PCO-API-Request-Rate-Count", "12")
            .with_header("retry-after", "20");
        assert_eq!(response.header("x-pco-api-request-rate-count"), Some("12"));
        assert_eq!(response.header("Retry-After"), Some("20"));
        assert_eq!(response.header("nope"), None);
    }

    #[test]
    fn a_get_carries_the_bearer_and_no_body() {
        let request = HttpRequest::get("https://example.test/x").with_bearer("tok");
        assert_eq!(request.method, Method::Get);
        assert_eq!(request.body, None);
        assert!(request
            .headers
            .iter()
            .any(|(k, v)| k == "authorization" && v == "Bearer tok"));
    }
}
