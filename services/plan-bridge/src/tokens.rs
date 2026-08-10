//! Keeping the access token fresh, on the read path.
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
    if now_s.saturating_add(lux_pco::oauth::REFRESH_SKEW_S) < conn.access_expires_at_s {
        return Ok(conn.access_token.clone());
    }

    let app = ctx.oauth().await.map_err(|e| {
        tracing::error!("oauth app unavailable: {e}");
        Refused::Unavailable
    })?;

    let tokens = match app.refresh(&ctx.http, &conn.refresh_token).await {
        Ok(t) => t,
        Err(lux_pco::Error::Unauthorized) => {
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

    if let Err(e) = store::set_tokens(
        ctx,
        sub,
        &tokens.access_token,
        &refresh_token,
        tokens.expires_at_s().unwrap_or(now_s),
        refresh_issued_at_s,
    )
    .await
    {
        // The refresh itself worked. Failing the read now would strand a
        // church mid-service over a bookkeeping error — serve the token, log
        // loudly, and let the next read try the write again.
        tracing::error!("refreshed token write failed, serving anyway: {e}");
    }

    Ok(tokens.access_token)
}

#[cfg(test)]
mod tests {
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

    /// The decision `fresh_access_token` makes before it touches anything.
    /// Pulled out so the "do we even need to refresh" rule is testable without
    /// AWS clients or a network.
    fn would_reuse(conn: &Connection, now_s: i64) -> bool {
        now_s.saturating_add(lux_pco::oauth::REFRESH_SKEW_S) < conn.access_expires_at_s
    }

    #[test]
    fn a_token_with_time_left_is_reused_rather_than_refreshed() {
        let c = conn(10_000);
        // An hour of life left: no round trip.
        assert!(would_reuse(&c, 6_400));
    }

    #[test]
    fn a_token_inside_the_skew_window_is_refreshed_before_it_bites() {
        let c = conn(10_000);
        // Four minutes left, inside the five-minute skew — refresh now, not
        // during the next request.
        assert!(!would_reuse(&c, 9_760));
        assert!(!would_reuse(&c, 10_001));
    }

    #[test]
    fn a_connection_with_no_recorded_expiry_always_refreshes() {
        // Written before the field existed, or by a token response that did
        // not say. One extra round trip beats a 401 mid-service.
        assert!(!would_reuse(&conn(0), 1));
    }
}
