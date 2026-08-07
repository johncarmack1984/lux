//! The authorization-code flow, and where the credentials come from.
//!
//! One OAuth application serves every church: Planning Center's own guidance
//! is to register it on a dedicated organization and reuse the single client
//! id and secret for each customer that connects. So the secret is *ours*, it
//! is server-side, and it never reaches an installed app.
//!
//! The dance, once per church:
//!
//! 1. The church's admin opens [`OAuthApp::authorize_url`] and approves the
//!    `services` scope.
//! 2. Planning Center redirects to the registered callback with a `code`.
//! 3. [`OAuthApp::exchange_code`] trades it for an access token (2 hours) and
//!    a refresh token (good for up to 90 days after issuance).
//! 4. [`OAuthApp::refresh`] renews both, well before
//!    [`Tokens::needs_refresh`] says the access token is about to expire.
//!
//! **The redirect URI must match the registration byte for byte**, port and
//! all, so the two live here as constants and travel as a *field* on
//! [`OAuthApp`] — never as a literal at a call site, where dev and prod would
//! eventually disagree.
//!
//! Credentials: in production the Lambda reads them from Secrets Manager at
//! [`SECRET_PATH`]; locally they come from the environment
//! ([`CLIENT_ID_ENV`] / [`CLIENT_SECRET_ENV`]) and their absence is a clean
//! [`Error::NotConfigured`], not a panic on a Sunday.

use std::fmt;

use serde::Deserialize;

use crate::error::Error;
use crate::http::{Http, HttpRequest};

pub const AUTHORIZE_URL: &str = "https://api.planningcenteronline.com/oauth/authorize";
pub const TOKEN_URL: &str = "https://api.planningcenteronline.com/oauth/token";

/// The only scope the bridge asks for. Planning Center has no finer grain than
/// this — see the crate docs on why read-only is enforced in code instead.
pub const SCOPE: &str = "services";

/// The registered production callback. Rides the existing web-auth domain; the
/// bridge Lambda serves this path.
pub const REDIRECT_URI_PROD: &str = "https://auth.lux.johncarmack.com/pco/callback";

/// The registered local-development callback. The port is part of the
/// registration — a dev server on any other port will be refused by Planning
/// Center, which is the intended behaviour.
pub const REDIRECT_URI_DEV: &str = "http://localhost:8474/pco/callback";

/// Where production keeps the client id and secret: an AWS Secrets Manager
/// secret holding `{"client_id": …, "client_secret": …}`. Named here so the
/// Lambda, the Terraform that grants it, and this crate all quote one string.
pub const SECRET_PATH: &str = "/lux/bridge/prod/pco-oauth";

pub const CLIENT_ID_ENV: &str = "LUX_PCO_CLIENT_ID";
pub const CLIENT_SECRET_ENV: &str = "LUX_PCO_CLIENT_SECRET";
/// Optional override; defaults to [`REDIRECT_URI_DEV`] outside production.
pub const REDIRECT_URI_ENV: &str = "LUX_PCO_REDIRECT_URI";

/// Refresh this long before the access token actually expires, so a token
/// never dies between the check and the request it was checked for.
pub const REFRESH_SKEW_S: i64 = 300;

/// The registered OAuth application: one per *integration*, not per church.
#[derive(Clone)]
pub struct OAuthApp {
    client_id: String,
    client_secret: String,
    redirect_uri: String,
}

/// Redacted on purpose: the secret must never reach a log line.
impl fmt::Debug for OAuthApp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthApp")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .field("redirect_uri", &self.redirect_uri)
            .finish()
    }
}

impl OAuthApp {
    pub fn new(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        redirect_uri: impl Into<String>,
    ) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            redirect_uri: redirect_uri.into(),
        }
    }

    /// Local development: credentials from the environment, callback defaulting
    /// to [`REDIRECT_URI_DEV`].
    pub fn from_env() -> Result<Self, Error> {
        Self::from_vars(
            std::env::var(CLIENT_ID_ENV).ok(),
            std::env::var(CLIENT_SECRET_ENV).ok(),
            std::env::var(REDIRECT_URI_ENV).ok(),
        )
    }

    /// The env-free half of [`from_env`](Self::from_env), so the
    /// not-configured behaviour is a test rather than a story.
    pub fn from_vars(
        client_id: Option<String>,
        client_secret: Option<String>,
        redirect_uri: Option<String>,
    ) -> Result<Self, Error> {
        let client_id = client_id
            .filter(|v| !v.trim().is_empty())
            .ok_or(Error::NotConfigured(CLIENT_ID_ENV))?;
        let client_secret = client_secret
            .filter(|v| !v.trim().is_empty())
            .ok_or(Error::NotConfigured(CLIENT_SECRET_ENV))?;
        let redirect_uri = redirect_uri
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| REDIRECT_URI_DEV.to_owned());
        Ok(Self::new(client_id, client_secret, redirect_uri))
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    /// Where to send the church's administrator to approve the connection.
    ///
    /// `state` is the caller's CSRF token: mint it per attempt, keep it, and
    /// refuse a callback that comes back with anything else.
    pub fn authorize_url(&self, state: &str) -> String {
        format!(
            "{AUTHORIZE_URL}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}",
            encode(&self.client_id),
            encode(&self.redirect_uri),
            encode(SCOPE),
            encode(state),
        )
    }

    /// Trade the callback's `code` for tokens.
    pub async fn exchange_code<H: Http + ?Sized>(
        &self,
        http: &H,
        code: &str,
    ) -> Result<Tokens, Error> {
        let body = form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
            ("redirect_uri", &self.redirect_uri),
        ]);
        self.post_token(http, body).await
    }

    /// Renew an access token. Planning Center issues a new refresh token with
    /// it; store both, or the 90-day clock is the last thing you'll hear from
    /// this church's connection.
    pub async fn refresh<H: Http + ?Sized>(
        &self,
        http: &H,
        refresh_token: &str,
    ) -> Result<Tokens, Error> {
        let body = form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
        ]);
        self.post_token(http, body).await
    }

    async fn post_token<H: Http + ?Sized>(&self, http: &H, body: String) -> Result<Tokens, Error> {
        let response = http.send(HttpRequest::form(TOKEN_URL, body)).await?;
        if response.status == 401 {
            return Err(Error::Unauthorized);
        }
        if !(200..300).contains(&response.status) {
            return Err(Error::Status {
                status: response.status,
                // The token endpoint answers with a small OAuth error object;
                // it holds no credential of ours.
                detail: truncate(&response.body, 300),
            });
        }
        serde_json::from_str(&response.body).map_err(|e| Error::Decode(e.to_string()))
    }
}

/// What the token endpoint answers with.
#[derive(Clone, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    #[serde(default)]
    pub token_type: String,
    /// Seconds from `created_at`. Planning Center issues 7200.
    #[serde(default)]
    pub expires_in: i64,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub scope: String,
    /// Unix seconds at issuance, per Planning Center's response.
    #[serde(default)]
    pub created_at: i64,
}

/// Redacted: neither token may reach a log line.
impl fmt::Debug for Tokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tokens")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("token_type", &self.token_type)
            .field("expires_in", &self.expires_in)
            .field("scope", &self.scope)
            .field("created_at", &self.created_at)
            .finish()
    }
}

impl Tokens {
    /// Unix seconds at which the access token stops working, when the response
    /// said enough to know.
    pub fn expires_at_s(&self) -> Option<i64> {
        (self.created_at > 0 && self.expires_in > 0)
            .then(|| self.created_at.saturating_add(self.expires_in))
    }

    /// Whether to refresh before using this token.
    ///
    /// A token whose lifetime we cannot compute counts as needing a refresh:
    /// the cost is one extra round trip at connect, and the alternative is
    /// discovering the expiry from a 401 in the middle of a service.
    pub fn needs_refresh(&self, now_s: i64) -> bool {
        match self.expires_at_s() {
            Some(expires_at) => now_s.saturating_add(REFRESH_SKEW_S) >= expires_at,
            None => true,
        }
    }
}

/// `a=1&b=2`, percent-encoded. Written out rather than pulled in: the whole
/// need is five pairs of ASCII, and a dependency for it would have to earn its
/// place in `cargo deny` forever.
fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Percent-encode everything outside RFC 3986's unreserved set. Spaces become
/// `%20` rather than `+`; both are legal in a form body and only one of them
/// is also correct in a URL query, so this encoder can serve both.
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(*byte));
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn truncate(body: &str, max: usize) -> String {
    match body.char_indices().nth(max) {
        Some((end, _)) => body[..end].to_owned(),
        None => body.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> OAuthApp {
        OAuthApp::new("cid", "csecret", REDIRECT_URI_PROD)
    }

    #[test]
    fn the_authorize_url_encodes_the_registered_callback() {
        assert_eq!(
            app().authorize_url("s-1"),
            "https://api.planningcenteronline.com/oauth/authorize\
             ?client_id=cid\
             &redirect_uri=https%3A%2F%2Fauth.lux.johncarmack.com%2Fpco%2Fcallback\
             &response_type=code&scope=services&state=s-1"
        );
    }

    #[test]
    fn the_registered_callbacks_are_exactly_these() {
        // Both are registered with Planning Center byte for byte. Changing
        // either here without re-registering breaks every connect.
        assert_eq!(
            REDIRECT_URI_PROD,
            "https://auth.lux.johncarmack.com/pco/callback"
        );
        assert_eq!(REDIRECT_URI_DEV, "http://localhost:8474/pco/callback");
        assert_eq!(SECRET_PATH, "/lux/bridge/prod/pco-oauth");
    }

    #[test]
    fn missing_credentials_are_a_clean_error_naming_what_to_set() {
        let err = OAuthApp::from_vars(None, Some("s".into()), None).unwrap_err();
        assert_eq!(err.to_string(), "LUX_PCO_CLIENT_ID is not set");

        // An empty or whitespace value is "unset", not a credential.
        let err = OAuthApp::from_vars(Some("id".into()), Some("  ".into()), None).unwrap_err();
        assert_eq!(err.to_string(), "LUX_PCO_CLIENT_SECRET is not set");
    }

    #[test]
    fn local_development_defaults_to_the_dev_callback() {
        let app = OAuthApp::from_vars(Some("id".into()), Some("secret".into()), None).unwrap();
        assert_eq!(app.redirect_uri(), REDIRECT_URI_DEV);

        let app = OAuthApp::from_vars(
            Some("id".into()),
            Some("secret".into()),
            Some(REDIRECT_URI_PROD.into()),
        )
        .unwrap();
        assert_eq!(app.redirect_uri(), REDIRECT_URI_PROD);
    }

    #[test]
    fn neither_the_secret_nor_a_token_survives_a_debug_print() {
        assert!(!format!("{:?}", app()).contains("csecret"));
        let tokens = Tokens {
            access_token: "at-secret".into(),
            token_type: "bearer".into(),
            expires_in: 7200,
            refresh_token: "rt-secret".into(),
            scope: "services".into(),
            created_at: 1_000,
        };
        let printed = format!("{tokens:?}");
        assert!(!printed.contains("at-secret"));
        assert!(!printed.contains("rt-secret"));
        assert!(printed.contains("services"));
    }

    #[test]
    fn a_token_is_refreshed_before_it_expires_not_after() {
        let tokens = Tokens {
            access_token: "at".into(),
            token_type: "bearer".into(),
            expires_in: 7200,
            refresh_token: "rt".into(),
            scope: "services".into(),
            created_at: 10_000,
        };
        assert_eq!(tokens.expires_at_s(), Some(17_200));
        assert!(!tokens.needs_refresh(16_000));
        // Inside the skew window, while the token still technically works.
        assert!(tokens.needs_refresh(16_900));
        assert!(tokens.needs_refresh(20_000));
    }

    #[test]
    fn a_token_with_no_lifetime_always_refreshes() {
        let tokens = Tokens {
            access_token: "at".into(),
            token_type: String::new(),
            expires_in: 0,
            refresh_token: String::new(),
            scope: String::new(),
            created_at: 0,
        };
        assert_eq!(tokens.expires_at_s(), None);
        assert!(tokens.needs_refresh(0));
    }

    #[test]
    fn the_form_body_is_percent_encoded() {
        assert_eq!(
            form(&[
                ("grant_type", "refresh_token"),
                ("redirect_uri", REDIRECT_URI_DEV)
            ]),
            "grant_type=refresh_token&redirect_uri=http%3A%2F%2Flocalhost%3A8474%2Fpco%2Fcallback"
        );
        // The characters an OAuth code or secret can actually contain.
        assert_eq!(encode("a+b/c=d&e~f_g-h.i"), "a%2Bb%2Fc%3Dd%26e~f_g-h.i");
    }
}
