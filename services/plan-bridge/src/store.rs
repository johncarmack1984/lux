//! Token custody, in the `lux-sync` table's own partitions.
//!
//! Two item kinds, both keyed off the verified Cognito `sub` — never off
//! anything a request body said:
//!
//! - `pk = PCO#<sub>,        sk = CONN`  — the connection: the church's org,
//!   the access token and its expiry, and the refresh token. One per account.
//! - `pk = PCOSTATE#<state>, sk = STATE` — an in-flight connect attempt,
//!   carrying the `sub` that started it. Read-and-delete, and it self-expires
//!   via `ttl` so an abandoned browser tab leaves nothing behind.
//!
//! The refresh token is the sensitive thing here: it is a 90-day credential
//! for another company's data about a church. It lives in exactly one place,
//! is never returned by any route, and is never logged — the reason the whole
//! integration is server-side rather than in the app (see the plan's §3.2.1).
//!
//! The partitions are pinned by the role's `dynamodb:LeadingKeys` condition
//! (`infra/plan-bridge.tf`), so this service cannot read a user's setups even
//! if a bug here asked it to.

use std::collections::HashMap;

use aws_sdk_dynamodb::types::AttributeValue;

use crate::Ctx;

const CONN_SK: &str = "CONN";
const STATE_SK: &str = "STATE";

fn conn_pk(sub: &str) -> String {
    format!("PCO#{sub}")
}

fn state_pk(state: &str) -> String {
    format!("PCOSTATE#{state}")
}

/// A church's live connection to Planning Center.
#[derive(Debug, Clone)]
pub struct Connection {
    pub org_id: Option<String>,
    pub org_name: Option<String>,
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds at which the access token stops working.
    pub access_expires_at_s: i64,
    /// Unix milliseconds at which the church first authorized.
    pub connected_at_ms: i64,
    /// Unix seconds at which the current refresh token was issued. Planning
    /// Center's refresh tokens are good for 90 days from issuance, so this is
    /// what tells a surface "reconnect" *before* a Sunday finds out for it.
    pub refresh_issued_at_s: i64,
}

/// Planning Center's documented refresh-token lifetime. Used only to warn
/// early — the authority is always what the token endpoint answers.
pub const REFRESH_LIFETIME_S: i64 = 90 * 24 * 60 * 60;

/// Warn this long before the refresh token's 90 days are up, so a church is
/// told to reconnect on a weekday rather than mid-service.
pub const RECONNECT_WARNING_S: i64 = 7 * 24 * 60 * 60;

impl Connection {
    /// Whether the stored refresh token is close enough to its ceiling that
    /// the surface should ask for a reconnect.
    pub fn needs_reconnect(&self, now_s: i64) -> bool {
        if self.refresh_issued_at_s <= 0 {
            // An unknown issuance date is not evidence of a problem; the next
            // refresh will either work or fail loudly.
            return false;
        }
        now_s.saturating_add(RECONNECT_WARNING_S)
            >= self.refresh_issued_at_s.saturating_add(REFRESH_LIFETIME_S)
    }
}

pub async fn get_connection(ctx: &Ctx, sub: &str) -> Result<Option<Connection>, String> {
    let out = ctx
        .ddb
        .get_item()
        .table_name(&ctx.table)
        .key("pk", AttributeValue::S(conn_pk(sub)))
        .key("sk", AttributeValue::S(CONN_SK.into()))
        .send()
        .await
        .map_err(|e| format!("connection read failed: {e}"))?;

    let Some(item) = out.item else {
        return Ok(None);
    };
    let access_token = string(&item, "accessToken").unwrap_or_default();
    let refresh_token = string(&item, "refreshToken").unwrap_or_default();
    // A half-written item is not a connection. Treating it as one would send a
    // church into a read that 401s with no way to recover but reconnecting —
    // which is exactly what `None` already tells the surface to do.
    if access_token.is_empty() || refresh_token.is_empty() {
        tracing::warn!("connection item is missing a token; treating as disconnected");
        return Ok(None);
    }
    Ok(Some(Connection {
        org_id: string(&item, "orgId"),
        org_name: string(&item, "orgName"),
        access_token,
        refresh_token,
        access_expires_at_s: number(&item, "accessExpiresAt").unwrap_or_default(),
        connected_at_ms: number(&item, "connectedAt").unwrap_or_default(),
        refresh_issued_at_s: number(&item, "refreshIssuedAt").unwrap_or_default(),
    }))
}

/// Write the whole connection — the connect path, after the code exchange.
pub async fn put_connection(ctx: &Ctx, sub: &str, conn: &Connection) -> Result<(), String> {
    let mut item: HashMap<String, AttributeValue> = HashMap::from([
        ("pk".into(), AttributeValue::S(conn_pk(sub))),
        ("sk".into(), AttributeValue::S(CONN_SK.into())),
        (
            "accessToken".into(),
            AttributeValue::S(conn.access_token.clone()),
        ),
        (
            "refreshToken".into(),
            AttributeValue::S(conn.refresh_token.clone()),
        ),
        (
            "accessExpiresAt".into(),
            AttributeValue::N(conn.access_expires_at_s.to_string()),
        ),
        (
            "connectedAt".into(),
            AttributeValue::N(conn.connected_at_ms.to_string()),
        ),
        (
            "refreshIssuedAt".into(),
            AttributeValue::N(conn.refresh_issued_at_s.to_string()),
        ),
    ]);
    if let Some(org_id) = &conn.org_id {
        item.insert("orgId".into(), AttributeValue::S(org_id.clone()));
    }
    if let Some(org_name) = &conn.org_name {
        item.insert("orgName".into(), AttributeValue::S(org_name.clone()));
    }

    ctx.ddb
        .put_item()
        .table_name(&ctx.table)
        .set_item(Some(item))
        .send()
        .await
        .map_err(|e| format!("connection write failed: {e}"))?;
    Ok(())
}

/// Record a refreshed token pair without touching the org fields.
///
/// An update rather than a put: a refresh happening while the church is being
/// renamed must not resurrect the old label, and a refresh must never be able
/// to *create* a connection that no one authorized.
pub async fn set_tokens(
    ctx: &Ctx,
    sub: &str,
    access_token: &str,
    refresh_token: &str,
    access_expires_at_s: i64,
    refresh_issued_at_s: i64,
) -> Result<(), String> {
    ctx.ddb
        .update_item()
        .table_name(&ctx.table)
        .key("pk", AttributeValue::S(conn_pk(sub)))
        .key("sk", AttributeValue::S(CONN_SK.into()))
        .condition_expression("attribute_exists(pk)")
        .update_expression(
            "SET accessToken = :a, refreshToken = :r, accessExpiresAt = :e, refreshIssuedAt = :i",
        )
        .expression_attribute_values(":a", AttributeValue::S(access_token.into()))
        .expression_attribute_values(":r", AttributeValue::S(refresh_token.into()))
        .expression_attribute_values(":e", AttributeValue::N(access_expires_at_s.to_string()))
        .expression_attribute_values(":i", AttributeValue::N(refresh_issued_at_s.to_string()))
        .send()
        .await
        .map_err(|e| format!("token update failed: {e}"))?;
    Ok(())
}

pub async fn delete_connection(ctx: &Ctx, sub: &str) -> Result<(), String> {
    ctx.ddb
        .delete_item()
        .table_name(&ctx.table)
        .key("pk", AttributeValue::S(conn_pk(sub)))
        .key("sk", AttributeValue::S(CONN_SK.into()))
        .send()
        .await
        .map_err(|e| format!("disconnect failed: {e}"))?;
    Ok(())
}

/// Bank an in-flight connect attempt. `ttl_secs` is how long the admin has to
/// finish at Planning Center.
pub async fn put_state(ctx: &Ctx, state: &str, sub: &str, ttl_secs: i64) -> Result<(), String> {
    ctx.ddb
        .put_item()
        .table_name(&ctx.table)
        .item("pk", AttributeValue::S(state_pk(state)))
        .item("sk", AttributeValue::S(STATE_SK.into()))
        .item("sub", AttributeValue::S(sub.into()))
        .item(
            "ttl",
            AttributeValue::N((now_secs() + ttl_secs).to_string()),
        )
        .send()
        .await
        .map_err(|e| format!("state write failed: {e}"))?;
    Ok(())
}

/// Consume a connect attempt, returning the `sub` that started it.
///
/// Delete-and-return, so a `state` is single-use: a replayed callback finds
/// nothing and is refused, which is the whole point of carrying one.
pub async fn take_state(ctx: &Ctx, state: &str) -> Result<Option<String>, String> {
    let out = ctx
        .ddb
        .delete_item()
        .table_name(&ctx.table)
        .key("pk", AttributeValue::S(state_pk(state)))
        .key("sk", AttributeValue::S(STATE_SK.into()))
        .return_values(aws_sdk_dynamodb::types::ReturnValue::AllOld)
        .send()
        .await
        .map_err(|e| format!("state take failed: {e}"))?;

    let Some(item) = out.attributes else {
        return Ok(None);
    };
    // DynamoDB's TTL sweep is eventual — an item can outlive its `ttl` by
    // hours. Enforce the window here so a stale callback is refused on time
    // rather than whenever the sweeper gets to it.
    if let Some(expiry) = number(&item, "ttl") {
        if now_secs() > expiry {
            return Ok(None);
        }
    }
    Ok(string(&item, "sub").filter(|s| !s.is_empty()))
}

fn string(item: &HashMap<String, AttributeValue>, key: &str) -> Option<String> {
    item.get(key)?.as_s().ok().cloned()
}

fn number(item: &HashMap<String, AttributeValue>, key: &str) -> Option<i64> {
    item.get(key)?.as_n().ok()?.parse().ok()
}

pub fn now_secs() -> i64 {
    now_millis() / 1_000
}

pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        // A clock before 1970 is a broken host, not a reason to abort a
        // service: zero reads as "unknown", which every caller here tolerates.
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(refresh_issued_at_s: i64) -> Connection {
        Connection {
            org_id: Some("org-9".into()),
            org_name: Some("Grace Chapel".into()),
            access_token: "at".into(),
            refresh_token: "rt".into(),
            access_expires_at_s: 0,
            connected_at_ms: 0,
            refresh_issued_at_s,
        }
    }

    #[test]
    fn a_church_is_warned_a_week_before_the_refresh_token_dies() {
        let issued = 1_000_000;
        let c = conn(issued);
        let ceiling = issued + REFRESH_LIFETIME_S;

        // Fresh: nothing to say.
        assert!(!c.needs_reconnect(issued));
        // Eight days out: still quiet.
        assert!(!c.needs_reconnect(ceiling - 8 * 24 * 60 * 60));
        // Inside the last week: say it now, on a weekday.
        assert!(c.needs_reconnect(ceiling - 6 * 24 * 60 * 60));
        assert!(c.needs_reconnect(ceiling + 1));
    }

    #[test]
    fn an_unknown_issuance_date_is_not_a_reconnect_prompt() {
        // Written by a build that predates the field; the next refresh decides.
        assert!(!conn(0).needs_reconnect(2_000_000));
    }

    #[test]
    fn the_partitions_are_the_ones_the_iam_policy_pins() {
        // These prefixes are duplicated in infra/plan-bridge.tf's LeadingKeys
        // condition. If one moves without the other, this service loses access
        // to its own items — so pin them here where a test can see it.
        assert_eq!(conn_pk("abc"), "PCO#abc");
        assert_eq!(state_pk("st-1"), "PCOSTATE#st-1");
    }
}
