//! Keeping the access token fresh on the read path — and handing it back at
//! the end.
//!
//! Planning Center's access tokens last two hours and their refresh tokens
//! ninety days, and a refresh mints a *new* refresh token — so the stored pair
//! has to be replaced, not just topped up, or the ninety-day clock quietly
//! becomes the last thing a church hears from its connection.
//!
//! The refresh happens lazily, on the read that needs it, rather than on a
//! schedule: a church that has not opened lux in a month should not be costing
//! a timer, and the read path is the only place where a stale token has any
//! consequence.

use lux_pco::{Http, OAuthApp};

use crate::store::{self, Connection};
use crate::Ctx;

/// Why a read cannot proceed. Both arms are things a surface can say something
/// useful about; neither is an internal error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The refresh token no longer works. Only the church's administrator can
    /// fix this, by authorizing again.
    Reconnect,
    /// Planning Center (or Secrets Manager) could not be reached. Nothing is
    /// wrong with the connection; try later.
    Unavailable,
}

/// Whether a stored access token is good for long enough to spend on a read.
///
/// The one place the rule lives, so the fast path below, the race recovery, and
/// the tests all ask the same question. A test that re-stated the comparison
/// could agree with a bug in it — the same reason `http::route` exists as a
/// function rather than a match a test re-writes.
fn is_usable(conn: &Connection, now_s: i64) -> bool {
    now_s.saturating_add(lux_pco::oauth::REFRESH_SKEW_S) < conn.access_expires_at_s
}

/// An access token good for the next few minutes.
///
/// Returns the stored one when it is still comfortably valid, and otherwise
/// refreshes and persists the new pair before handing it back.
pub async fn fresh_access_token(
    ctx: &Ctx,
    sub: &str,
    conn: &Connection,
) -> Result<String, Refused> {
    let now_s = store::now_secs();
    if is_usable(conn, now_s) {
        return Ok(conn.access_token.clone());
    }

    let app = ctx.oauth().await.map_err(|e| {
        tracing::error!("oauth app unavailable: {e}");
        Refused::Unavailable
    })?;

    let tokens = match app.refresh(&ctx.http, &conn.refresh_token).await {
        Ok(t) => t,
        Err(lux_pco::Error::Unauthorized) => {
            // Two reads that overlap on an expired access token both refresh
            // with the same rotating token, and Planning Center honours one of
            // them — so "rejected" is ambiguous here. Look again before saying
            // the word: if the other read has already stored a working pair,
            // this connection is healthy and telling a church to reconnect
            // would be exactly the wrong sentence.
            if let Some(token) = token_from_a_concurrent_refresh(ctx, sub, conn).await {
                return Ok(token);
            }
            // The refresh token is spent, revoked, or ninety days old. The
            // stored pair is now worthless; say so rather than retrying it on
            // every poll for the rest of the service.
            tracing::warn!("pco refresh rejected; reconnect required");
            return Err(Refused::Reconnect);
        }
        Err(e) => {
            tracing::error!("pco refresh failed: {e}");
            return Err(Refused::Unavailable);
        }
    };

    // Planning Center returns both halves. A response missing the refresh half
    // would leave us storing an empty credential, so keep the one that still
    // works rather than overwriting it with nothing.
    let refresh_token = if tokens.refresh_token.is_empty() {
        tracing::warn!("pco refresh response carried no refresh token; keeping the current one");
        conn.refresh_token.clone()
    } else {
        tokens.refresh_token.clone()
    };
    let refresh_issued_at_s = if tokens.created_at > 0 {
        tokens.created_at
    } else {
        now_s
    };

    match store::set_tokens(
        ctx,
        sub,
        &conn.refresh_token,
        &tokens.access_token,
        &refresh_token,
        tokens.expires_at_s().unwrap_or(now_s),
        refresh_issued_at_s,
    )
    .await
    {
        Ok(true) => {}
        // The stored pair moved under this write: another read rotated it, or
        // the church disconnected. Either way theirs is the current truth and
        // this one must not put an older pair back.
        Ok(false) => tracing::info!("a concurrent refresh already stored a newer pair"),
        // The refresh itself worked. Failing the read now would strand a
        // church mid-service over a bookkeeping error — serve the token, log
        // loudly, and let the next read try the write again.
        Err(e) => tracing::error!("refreshed token write failed, serving anyway: {e}"),
    }

    Ok(tokens.access_token)
}

/// The access token another in-flight read just stored, when there is one.
///
/// Only consulted after Planning Center refuses a refresh: a rotating refresh
/// token that has already been spent by a concurrent read looks exactly like a
/// revoked one from here, and the stored pair is what tells the two apart.
async fn token_from_a_concurrent_refresh(
    ctx: &Ctx,
    sub: &str,
    refreshed_with: &Connection,
) -> Option<String> {
    let current = match store::get_connection(ctx, sub).await {
        Ok(Some(c)) => c,
        // No connection at all: the church disconnected mid-read, which is a
        // reconnect in every sense that matters to the caller.
        Ok(None) => return None,
        Err(e) => {
            tracing::error!("post-refresh connection read failed: {e}");
            return None;
        }
    };
    if current.refresh_token == refreshed_with.refresh_token {
        // Nothing moved, so nothing raced. The token really is spent.
        return None;
    }
    if !is_usable(&current, store::now_secs()) {
        // Rotated, but not into something this read can spend. Fall through to
        // the honest answer rather than serving a token about to expire.
        return None;
    }
    tracing::info!("refresh raced a concurrent one; using the pair it stored");
    Some(current.access_token)
}

// --- giving it back -----------------------------------------------------------

/// What became of a revocation attempt.
///
/// There is no error arm, and [`revoke`] returns this rather than a `Result` on
/// purpose: its callers are a disconnect and an account deletion, and both are
/// operations that must finish. A deletion that refused to complete because
/// Planning Center was unreachable would leave lux holding the very credential
/// the church just asked it to let go of — the opposite of what was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Revoked {
    /// Nothing was connected. The ordinary case for an account that never used
    /// the bridge, and for a second disconnect.
    NothingToRevoke,
    /// Planning Center accepted it — or had already revoked it, which answers
    /// the same way and means the same thing.
    Done,
    /// The attempt did not land. The stored row is deleted regardless, so lux
    /// stops holding the token; the grant itself may outlive it at Planning
    /// Center until the church revokes lux in their own settings or the 90 days
    /// run out. Logged loudly here because nothing downstream can see it.
    Failed,
}

/// Hand the church's refresh token back to Planning Center.
///
/// Called before the stored row is deleted, never after: once the row is gone
/// the token is gone with it, and a credential nobody can revoke is exactly the
/// thing this exists to prevent. Doing it in this order also makes a failed
/// delete harmless — the token is already dead by then.
pub async fn revoke<H: Http + ?Sized>(
    app: &OAuthApp,
    http: &H,
    conn: Option<&Connection>,
) -> Revoked {
    let Some(conn) = conn else {
        return Revoked::NothingToRevoke;
    };
    match app.revoke(http, &conn.refresh_token).await {
        Ok(()) => {
            tracing::info!("planning center authorization revoked");
            Revoked::Done
        }
        Err(e) => {
            // Never the token itself: the error type redacts it, and this line
            // must stay safe to read in CloudWatch.
            tracing::error!("pco revoke failed, deleting the stored token anyway: {e}");
            Revoked::Failed
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use lux_pco::http::{BoxFuture, HttpRequest, HttpResponse};

    use super::*;

    fn conn(access_expires_at_s: i64) -> Connection {
        Connection {
            org_id: None,
            org_name: None,
            access_token: "at".into(),
            refresh_token: "rt".into(),
            access_expires_at_s,
            connected_at_ms: 0,
            refresh_issued_at_s: 0,
        }
    }

    /// A transport that answers with one canned outcome and remembers whether
    /// it was asked anything at all. "Was Planning Center called?" is half of
    /// what the tests below are about: a disconnect on an account that never
    /// connected must not send a request carrying an empty token.
    struct Fake {
        answer: Result<HttpResponse, lux_pco::Error>,
        calls: Mutex<Vec<HttpRequest>>,
    }

    impl Fake {
        fn answering(status: u16) -> Self {
            Self {
                answer: Ok(HttpResponse::new(status, "{}")),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn offline() -> Self {
            Self {
                answer: Err(lux_pco::Error::Transport("no route to host".into())),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<HttpRequest> {
            self.calls.lock().map(|c| c.clone()).unwrap_or_default()
        }
    }

    impl Http for Fake {
        fn send(
            &self,
            request: HttpRequest,
        ) -> BoxFuture<'_, Result<HttpResponse, lux_pco::Error>> {
            if let Ok(mut calls) = self.calls.lock() {
                calls.push(request);
            }
            let answer = match &self.answer {
                Ok(response) => Ok(response.clone()),
                Err(e) => Err(lux_pco::Error::Transport(e.to_string())),
            };
            Box::pin(async move { answer })
        }
    }

    fn app() -> OAuthApp {
        OAuthApp::new("cid", "csecret", lux_pco::oauth::REDIRECT_URI_PROD)
    }

    #[tokio::test]
    async fn a_connected_church_has_its_token_handed_back() {
        let http = Fake::answering(200);
        let connection = conn(10_000);

        assert_eq!(
            revoke(&app(), &http, Some(&connection)).await,
            Revoked::Done
        );

        // The refresh token, at the revocation endpoint. Revoking that half is
        // what ends the 90-day credential — the whole point of the exercise.
        let calls = http.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].url, lux_pco::oauth::REVOKE_URL);
        let body = calls[0].body.clone().expect("a form body");
        assert!(body.contains("token=rt"), "{body}");
        assert!(body.contains("token_type_hint=refresh_token"), "{body}");
    }

    #[tokio::test]
    async fn an_account_that_never_connected_is_a_quiet_no_op() {
        // Deleting an account is the common case here: most accounts have no
        // Planning Center connection at all, and the deletion must not spend a
        // round trip — or send a request with an empty token in it — to learn
        // that.
        let http = Fake::answering(200);

        assert_eq!(revoke(&app(), &http, None).await, Revoked::NothingToRevoke);
        assert!(http.calls().is_empty(), "nothing should have been asked");
    }

    #[tokio::test]
    async fn a_failed_revocation_is_reported_but_never_fatal() {
        // Planning Center refusing, and Planning Center unreachable. Neither
        // may become an error: the caller is deleting an account, and the row
        // is deleted either way. `revoke` has no error arm to return, which is
        // the guarantee — this test pins the outcome it reports instead.
        let refused = Fake::answering(500);
        assert_eq!(
            revoke(&app(), &refused, Some(&conn(10_000))).await,
            Revoked::Failed
        );
        assert_eq!(refused.calls().len(), 1, "it did try");

        let offline = Fake::offline();
        assert_eq!(
            revoke(&app(), &offline, Some(&conn(10_000))).await,
            Revoked::Failed
        );
    }

    #[test]
    fn a_token_with_time_left_is_reused_rather_than_refreshed() {
        let c = conn(10_000);
        // An hour of life left: no round trip.
        assert!(is_usable(&c, 6_400));
    }

    #[test]
    fn a_token_inside_the_skew_window_is_refreshed_before_it_bites() {
        let c = conn(10_000);
        // Four minutes left, inside the five-minute skew — refresh now, not
        // during the next request.
        assert!(!is_usable(&c, 9_760));
        assert!(!is_usable(&c, 10_001));
    }

    #[test]
    fn a_connection_with_no_recorded_expiry_always_refreshes() {
        // Written before the field existed, or by a token response that did
        // not say. One extra round trip beats a 401 mid-service.
        assert!(!is_usable(&conn(0), 1));
    }

    #[test]
    fn the_skew_is_wide_enough_to_cover_a_read() {
        // The point of the skew is that a token handed out here survives the
        // Planning Center round trips the caller is about to make. Five
        // minutes; a value small enough to expire mid-read would turn this
        // module into the thing it exists to prevent.
        assert!(lux_pco::oauth::REFRESH_SKEW_S >= 60);
        let c = conn(10_000);
        assert!(is_usable(&c, 10_000 - lux_pco::oauth::REFRESH_SKEW_S - 1));
        assert!(!is_usable(&c, 10_000 - lux_pco::oauth::REFRESH_SKEW_S));
    }
}
