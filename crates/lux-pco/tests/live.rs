//! Verification against the **real** Planning Center OAuth server.
//!
//! Every test here is `#[ignore]`d: they need the network and the registered
//! application's credentials, so they are never part of the PR gate. Run them
//! by hand when the OAuth application changes, or when something about a
//! connection stops making sense:
//!
//! ```text
//! LUX_PCO_CLIENT_ID=… LUX_PCO_CLIENT_SECRET=… \
//!   cargo test -p lux-pco --test live -- --ignored --nocapture
//! ```
//!
//! Credentials come from the environment, which is what `OAuthApp::from_env`
//! already reads; in practice they come out of Secrets Manager:
//!
//! ```text
//! eval "$(aws secretsmanager get-secret-value --profile newearth-admin \
//!   --secret-id /lux/bridge/prod/pco-oauth --query SecretString --output text \
//!   | jq -r '"export LUX_PCO_CLIENT_ID=\(.client_id) LUX_PCO_CLIENT_SECRET=\(.client_secret)"')"
//! ```
//!
//! **What these can and cannot prove.** Planning Center redirects
//! `/oauth/authorize` to its login page *before* validating `client_id` or
//! `redirect_uri`, so no unauthenticated request can confirm that the callback
//! URIs are registered — that is only checked after a human signs in. What is
//! provable without a human is the half below: the token endpoint is reachable
//! over TLS, it answers in the shape this crate parses, a dead refresh token
//! produces exactly the error the bridge turns into "reconnect" rather than
//! into a 500 in the middle of a service, and the revocation endpoint — the
//! one account deletion depends on — is there and answers the way its
//! documentation says.

use lux_pco::{Error, OAuthApp, ReqwestHttp};

/// This crate's reqwest carries no crypto provider, so a process must install
/// one before its first TLS request — the same line every Lambda's `main` runs.
/// Idempotent, so each test can call it without coordinating.
fn install_tls() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

fn app() -> Option<OAuthApp> {
    install_tls();
    match OAuthApp::from_env() {
        Ok(app) => Some(app),
        Err(e) => {
            eprintln!("skipping: {e}");
            None
        }
    }
}

/// A refresh token that no longer works must come back as [`Error::Unauthorized`].
///
/// This is the single most consequential error path in the bridge: it is what
/// a church hits ninety days after connecting, and `tokens::fresh_access_token`
/// turns it into "reconnect" — a sentence an operator can act on — instead of a
/// 500. Pinning it against the live server means a change in Planning Center's
/// status code shows up here rather than on a Sunday morning.
#[tokio::test]
#[ignore = "hits the live Planning Center OAuth server"]
async fn a_dead_refresh_token_is_unauthorized_not_a_surprise() {
    let Some(app) = app() else { return };
    let http = ReqwestHttp::new().expect("transport");

    let err = app
        .refresh(&http, "definitely-not-a-real-refresh-token")
        .await
        .expect_err("a bogus refresh token must not succeed");

    eprintln!("live refresh error: {err}");
    assert!(
        matches!(err, Error::Unauthorized),
        "expected Unauthorized so the bridge can say 'reconnect'; got {err:?}"
    );
}

/// An authorization code that was never issued must fail the same way.
///
/// The connect path's failure, rather than the refresh path's: it decides
/// whether a church sees "start again from lux" or a blank error page.
#[tokio::test]
#[ignore = "hits the live Planning Center OAuth server"]
async fn a_bogus_authorization_code_is_refused_cleanly() {
    let Some(app) = app() else { return };
    let http = ReqwestHttp::new().expect("transport");

    let err = app
        .exchange_code(&http, "not-a-real-authorization-code")
        .await
        .expect_err("a bogus code must not succeed");

    eprintln!("live exchange error: {err}");
    // Whatever Planning Center calls it, it must arrive as a typed error this
    // crate can name — never a decode panic and never a success.
    assert!(
        matches!(err, Error::Unauthorized | Error::Status { .. }),
        "expected a typed refusal; got {err:?}"
    );
}

/// The revocation endpoint exists, accepts this application's credentials, and
/// treats a token it has never seen as already gone.
///
/// Account deletion leans on both halves of that: the endpoint has to be there
/// (it is the only way lux can end its own access without a church visiting
/// Planning Center's settings), and a 200 for an unknown token is what makes a
/// retried deletion — or a second disconnect — a no-op rather than a failure.
/// Documented as "a successful revocation returns 200, including for a token
/// that was already revoked or is invalid"; pinned here so a change in that
/// behaviour surfaces during a runbook pass instead of leaving a live 90-day
/// credential behind a deleted account.
#[tokio::test]
#[ignore = "hits the live Planning Center OAuth server"]
async fn revoking_a_token_planning_center_never_issued_still_answers_ok() {
    let Some(app) = app() else { return };
    let http = ReqwestHttp::new().expect("transport");

    let result = app
        .revoke(&http, "definitely-not-a-real-refresh-token")
        .await;
    match &result {
        Ok(()) => eprintln!("live revoke: 200, as documented"),
        Err(e) => eprintln!("live revoke error: {e}"),
    }
    assert!(
        result.is_ok(),
        "the documented answer is 200 even for an unknown token; got {result:?}"
    );
}

/// The consent URL this crate builds is one Planning Center actually accepts
/// as far as its login page — i.e. it is well-formed and reaches the flow.
///
/// Deliberately *not* a claim that the redirect is registered: Planning Center
/// does not check that until after login, so this proves reachability and
/// nothing more. The registration itself is confirmed by the one interactive
/// step in the runbook.
#[tokio::test]
#[ignore = "hits the live Planning Center OAuth server"]
async fn the_consent_url_reaches_planning_centers_login() {
    let Some(app) = app() else { return };
    let url = app.authorize_url("verification-probe");
    eprintln!("authorize url host check: {}", &url[..60.min(url.len())]);

    let response = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client")
        .get(&url)
        .send()
        .await
        .expect("authorize endpoint is reachable");

    let status = response.status().as_u16();
    let location = response
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    eprintln!("authorize -> HTTP {status}, location {location}");

    assert_eq!(status, 302, "expected a redirect into the login flow");
    assert!(
        location.contains("planningcenteronline.com"),
        "expected to be sent to Planning Center; got {location}"
    );
}
