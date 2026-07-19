//! The Apple↔Cognito link store: two mirrored items per link in the `lux-sync`
//! table, written transactionally.
//!
//! - `pk = APPLE#<apple_sub>,   sk = LINK` — forward: which Cognito user this
//!   Apple credential signs into (plus the revocable Apple refresh token).
//! - `pk = APPLELINK#<sub>,     sk = LINK` — reverse: which Apple credential
//!   the Cognito user is bound to (revoke/deletion's lookup).
//!
//! Both partitions are deliberately disjoint from the sync data's `USER#<sub>`
//! partitions, so sync's list query never sees them and account deletion's
//! partition wipe never races the revoke flow — cleaning these up is the
//! revoke route's job. The IAM policy pins this service to exactly these key
//! prefixes (`dynamodb:LeadingKeys`).

use std::collections::HashMap;

use aws_sdk_dynamodb::types::{AttributeValue, Delete, Put, TransactWriteItem};

use crate::Ctx;

/// One Apple↔Cognito link, as read from the forward item.
#[derive(Debug)]
pub struct Link {
    pub username: String,
    pub sub: String,
    pub apple_refresh_token: String,
}

fn forward_pk(apple_sub: &str) -> String {
    format!("APPLE#{apple_sub}")
}

fn reverse_pk(sub: &str) -> String {
    format!("APPLELINK#{sub}")
}

const LINK_SK: &str = "LINK";

pub async fn get_link(ctx: &Ctx, apple_sub: &str) -> Result<Option<Link>, String> {
    let out = ctx
        .ddb
        .get_item()
        .table_name(&ctx.table)
        .key("pk", AttributeValue::S(forward_pk(apple_sub)))
        .key("sk", AttributeValue::S(LINK_SK.into()))
        .send()
        .await
        .map_err(|e| format!("link get failed: {e}"))?;
    let Some(item) = out.item else {
        return Ok(None);
    };
    let s = |k: &str| -> Result<String, String> {
        item.get(k)
            .and_then(|v| v.as_s().ok())
            .cloned()
            .ok_or_else(|| format!("link item missing {k}"))
    };
    Ok(Some(Link {
        username: s("username")?,
        sub: s("sub")?,
        apple_refresh_token: s("appleRefreshToken")?,
    }))
}

/// The linked Apple `sub` for a Cognito user, if any.
pub async fn get_reverse(ctx: &Ctx, sub: &str) -> Result<Option<String>, String> {
    let out = ctx
        .ddb
        .get_item()
        .table_name(&ctx.table)
        .key("pk", AttributeValue::S(reverse_pk(sub)))
        .key("sk", AttributeValue::S(LINK_SK.into()))
        .send()
        .await
        .map_err(|e| format!("reverse link get failed: {e}"))?;
    Ok(out
        .item
        .and_then(|item| item.get("appleSub").and_then(|v| v.as_s().ok()).cloned()))
}

/// Write (or overwrite) both halves of a link transactionally.
#[allow(clippy::too_many_arguments)]
pub async fn put_link(
    ctx: &Ctx,
    apple_sub: &str,
    username: &str,
    sub: &str,
    apple_refresh_token: &str,
    email_seen: Option<&str>,
    name_seen: Option<&str>,
) -> Result<(), String> {
    let mut forward: HashMap<String, AttributeValue> = HashMap::from([
        ("pk".into(), AttributeValue::S(forward_pk(apple_sub))),
        ("sk".into(), AttributeValue::S(LINK_SK.into())),
        ("username".into(), AttributeValue::S(username.into())),
        ("sub".into(), AttributeValue::S(sub.into())),
        (
            "appleRefreshToken".into(),
            AttributeValue::S(apple_refresh_token.into()),
        ),
        (
            "createdAt".into(),
            AttributeValue::N(now_millis().to_string()),
        ),
    ]);
    // First-authorization extras: Apple only ever sends these once, so they are
    // recorded on the link or nowhere.
    if let Some(email) = email_seen {
        forward.insert("emailSeen".into(), AttributeValue::S(email.into()));
    }
    if let Some(name) = name_seen {
        forward.insert("nameSeen".into(), AttributeValue::S(name.into()));
    }

    let reverse: HashMap<String, AttributeValue> = HashMap::from([
        ("pk".into(), AttributeValue::S(reverse_pk(sub))),
        ("sk".into(), AttributeValue::S(LINK_SK.into())),
        ("appleSub".into(), AttributeValue::S(apple_sub.into())),
    ]);

    // Links are written once (re-auth refreshes ride `set_refresh_token`), so
    // both puts insist on inserting: a concurrent duplicate first-link loses
    // cleanly instead of half-overwriting a 1:1 pair.
    let put = |item| {
        Put::builder()
            .table_name(&ctx.table)
            .set_item(Some(item))
            .condition_expression("attribute_not_exists(pk)")
            .build()
            .map_err(|e| format!("link put malformed: {e}"))
    };
    ctx.ddb
        .transact_write_items()
        .transact_items(TransactWriteItem::builder().put(put(forward)?).build())
        .transact_items(TransactWriteItem::builder().put(put(reverse)?).build())
        .send()
        .await
        .map_err(|e| format!("link write failed: {e}"))?;
    Ok(())
}

/// Update the stored revocable Apple token on a re-auth (best-effort caller).
pub async fn set_refresh_token(ctx: &Ctx, apple_sub: &str, token: &str) -> Result<(), String> {
    ctx.ddb
        .update_item()
        .table_name(&ctx.table)
        .key("pk", AttributeValue::S(forward_pk(apple_sub)))
        .key("sk", AttributeValue::S(LINK_SK.into()))
        .update_expression("SET appleRefreshToken = :t")
        .expression_attribute_values(":t", AttributeValue::S(token.into()))
        .condition_expression("attribute_exists(pk)")
        .send()
        .await
        .map_err(|e| format!("refresh token update failed: {e}"))?;
    Ok(())
}

/// Drop both halves of a link transactionally (after a successful revoke).
pub async fn delete_link(ctx: &Ctx, apple_sub: &str, sub: &str) -> Result<(), String> {
    let delete = |pk: String| {
        Delete::builder()
            .table_name(&ctx.table)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S(LINK_SK.into()))
            .build()
            .map_err(|e| format!("link delete malformed: {e}"))
    };
    ctx.ddb
        .transact_write_items()
        .transact_items(
            TransactWriteItem::builder()
                .delete(delete(forward_pk(apple_sub))?)
                .build(),
        )
        .transact_items(
            TransactWriteItem::builder()
                .delete(delete(reverse_pk(sub))?)
                .build(),
        )
        .send()
        .await
        .map_err(|e| format!("link delete failed: {e}"))?;
    Ok(())
}

fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// --- Device pairing (docs/claim-code-pairing.md) -------------------------------
//
// Three more disjoint partitions, same LeadingKeys discipline as the links:
// - `pk = PAIR#<sha256(device_code)>, sk = PAIR` — one grant attempt, keyed by
//   the hash so the secret itself is never at rest.
// - `pk = PAIRIP#<public_ip>, sk = <createdAt>#<device_id>` — the approve
//   screen's same-egress pending list.
// - `pk = DEVICE#<sub>, sk = <device_id>` — the owner's paired-device registry.
// PAIR/PAIRIP items carry `ttl` (epoch seconds) and self-expire; DEVICE items
// don't.

/// One pairing grant, as read back from the `PAIR#` item. (The item carries
/// more — userCode, deviceName — surfaced here only once something reads them.)
#[derive(Debug)]
pub struct Pair {
    pub status: String,
    pub device_id: String,
    pub hostname: String,
    /// Set on approve.
    pub bound_username: Option<String>,
    pub bound_sub: Option<String>,
    pub setup_id: Option<String>,
    pub universe: Option<u16>,
    /// Epoch millis.
    pub created_at: i64,
    pub expires_at: i64,
    pub redeemed_at: Option<i64>,
    pub pub_ip: String,
}

impl Pair {
    pub fn expired(&self) -> bool {
        now_millis() >= self.expires_at
    }
}

fn pair_pk(pair_ref: &str) -> String {
    format!("PAIR#{pair_ref}")
}

fn pair_ip_pk(pub_ip: &str) -> String {
    format!("PAIRIP#{pub_ip}")
}

fn device_pk(sub: &str) -> String {
    format!("DEVICE#{sub}")
}

const PAIR_SK: &str = "PAIR";

/// Register a grant attempt: the `PAIR#` item and its `PAIRIP#` pending-list
/// row, transactionally. `pair_ref` is the hex sha256 of the device code.
#[allow(clippy::too_many_arguments)]
pub async fn create_pair(
    ctx: &Ctx,
    pair_ref: &str,
    user_code: &str,
    device_id: &str,
    hostname: &str,
    mac_tail: &str,
    version: &str,
    arch: &str,
    pub_ip: &str,
    expires_in_secs: i64,
) -> Result<(), String> {
    let created_at = now_millis();
    let expires_at = created_at + expires_in_secs * 1000;
    // DynamoDB TTL is epoch seconds; keep expired rows a day for debugging.
    let ttl = expires_at / 1000 + 86_400;

    let s = |v: &str| AttributeValue::S(v.into());
    let n = |v: i64| AttributeValue::N(v.to_string());

    let pair: HashMap<String, AttributeValue> = HashMap::from([
        ("pk".into(), s(&pair_pk(pair_ref))),
        ("sk".into(), s(PAIR_SK)),
        ("status".into(), s("pending")),
        ("userCode".into(), s(user_code)),
        ("deviceId".into(), s(device_id)),
        ("hostname".into(), s(hostname)),
        ("macTail".into(), s(mac_tail)),
        ("version".into(), s(version)),
        ("arch".into(), s(arch)),
        ("pubIp".into(), s(pub_ip)),
        ("createdAt".into(), n(created_at)),
        ("expiresAt".into(), n(expires_at)),
        ("ttl".into(), n(ttl)),
    ]);
    let row: HashMap<String, AttributeValue> = HashMap::from([
        ("pk".into(), s(&pair_ip_pk(pub_ip))),
        ("sk".into(), s(&format!("{created_at}#{device_id}"))),
        ("pairRef".into(), s(pair_ref)),
        ("userCode".into(), s(user_code)),
        ("hostname".into(), s(hostname)),
        ("macTail".into(), s(mac_tail)),
        ("version".into(), s(version)),
        ("arch".into(), s(arch)),
        ("createdAt".into(), n(created_at)),
        ("expiresAt".into(), n(expires_at)),
        ("ttl".into(), n(ttl)),
    ]);

    let put = |item| {
        Put::builder()
            .table_name(&ctx.table)
            .set_item(Some(item))
            .condition_expression("attribute_not_exists(pk)")
            .build()
            .map_err(|e| format!("pair put malformed: {e}"))
    };
    ctx.ddb
        .transact_write_items()
        .transact_items(TransactWriteItem::builder().put(put(pair)?).build())
        .transact_items(TransactWriteItem::builder().put(put(row)?).build())
        .send()
        .await
        .map_err(|e| format!("pair write failed: {e}"))?;
    Ok(())
}

pub async fn get_pair(ctx: &Ctx, pair_ref: &str) -> Result<Option<Pair>, String> {
    let out = ctx
        .ddb
        .get_item()
        .table_name(&ctx.table)
        .key("pk", AttributeValue::S(pair_pk(pair_ref)))
        .key("sk", AttributeValue::S(PAIR_SK.into()))
        .send()
        .await
        .map_err(|e| format!("pair get failed: {e}"))?;
    let Some(item) = out.item else {
        return Ok(None);
    };
    let s = |k: &str| -> Result<String, String> {
        item.get(k)
            .and_then(|v| v.as_s().ok())
            .cloned()
            .ok_or_else(|| format!("pair item missing {k}"))
    };
    let opt_s = |k: &str| item.get(k).and_then(|v| v.as_s().ok()).cloned();
    let num = |k: &str| -> Result<i64, String> {
        item.get(k)
            .and_then(|v| v.as_n().ok())
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| format!("pair item missing {k}"))
    };
    let opt_n = |k: &str| {
        item.get(k)
            .and_then(|v| v.as_n().ok())
            .and_then(|v| v.parse::<i64>().ok())
    };
    Ok(Some(Pair {
        status: s("status")?,
        device_id: s("deviceId")?,
        hostname: s("hostname")?,
        bound_username: opt_s("boundUsername"),
        bound_sub: opt_s("boundSub"),
        setup_id: opt_s("setupId"),
        universe: opt_n("universe").map(|v| v as u16),
        created_at: num("createdAt")?,
        expires_at: num("expiresAt")?,
        redeemed_at: opt_n("redeemedAt"),
        pub_ip: s("pubIp")?,
    }))
}

/// Unexpired pending-list rows for one public IP, oldest first. Approval
/// deletes its row, so what's here is (at most a TTL-lag away from) pending.
pub async fn list_pending(
    ctx: &Ctx,
    pub_ip: &str,
) -> Result<Vec<HashMap<String, AttributeValue>>, String> {
    let out = ctx
        .ddb
        .query()
        .table_name(&ctx.table)
        .key_condition_expression("pk = :pk")
        .expression_attribute_values(":pk", AttributeValue::S(pair_ip_pk(pub_ip)))
        .send()
        .await
        .map_err(|e| format!("pending query failed: {e}"))?;
    let now = now_millis();
    Ok(out
        .items
        .unwrap_or_default()
        .into_iter()
        .filter(|item| {
            item.get("expiresAt")
                .and_then(|v| v.as_n().ok())
                .and_then(|v| v.parse::<i64>().ok())
                .is_some_and(|exp| exp > now)
        })
        .collect())
}

/// The owner's paired-device registry rows (`DEVICE#<sub>`), as stored.
pub async fn list_devices(
    ctx: &Ctx,
    sub: &str,
) -> Result<Vec<HashMap<String, AttributeValue>>, String> {
    let out = ctx
        .ddb
        .query()
        .table_name(&ctx.table)
        .key_condition_expression("pk = :pk")
        .expression_attribute_values(":pk", AttributeValue::S(device_pk(sub)))
        .send()
        .await
        .map_err(|e| format!("device query failed: {e}"))?;
    Ok(out.items.unwrap_or_default())
}

/// Approve a pending grant: bind it to the approver and the chosen setup,
/// retire its pending-list row, and record the device in the owner's registry
/// — one transaction. Fails (condition) if the grant isn't pending anymore.
#[allow(clippy::too_many_arguments)]
pub async fn approve_pair(
    ctx: &Ctx,
    pair_ref: &str,
    pair: &Pair,
    username: &str,
    sub: &str,
    setup_id: &str,
    universe: u16,
    device_name: &str,
) -> Result<(), String> {
    let now = now_millis();
    let s = |v: &str| AttributeValue::S(v.into());
    let n = |v: i64| AttributeValue::N(v.to_string());

    let update = aws_sdk_dynamodb::types::Update::builder()
        .table_name(&ctx.table)
        .key("pk", s(&pair_pk(pair_ref)))
        .key("sk", s(PAIR_SK))
        .update_expression(
            "SET #st = :approved, boundUsername = :u, boundSub = :sub, \
             setupId = :setup, universe = :uni, deviceName = :name, approvedAt = :now",
        )
        .condition_expression("#st = :pending AND expiresAt > :now")
        .expression_attribute_names("#st", "status")
        .expression_attribute_values(":approved", s("approved"))
        .expression_attribute_values(":pending", s("pending"))
        .expression_attribute_values(":u", s(username))
        .expression_attribute_values(":sub", s(sub))
        .expression_attribute_values(":setup", s(setup_id))
        .expression_attribute_values(":uni", n(universe as i64))
        .expression_attribute_values(":name", s(device_name))
        .expression_attribute_values(":now", n(now))
        .build()
        .map_err(|e| format!("pair approve malformed: {e}"))?;

    let drop_row = Delete::builder()
        .table_name(&ctx.table)
        .key("pk", s(&pair_ip_pk(&pair.pub_ip)))
        .key("sk", s(&format!("{}#{}", pair.created_at, pair.device_id)))
        .build()
        .map_err(|e| format!("pending-row delete malformed: {e}"))?;

    let registry: HashMap<String, AttributeValue> = HashMap::from([
        ("pk".into(), s(&device_pk(sub))),
        ("sk".into(), s(&pair.device_id)),
        ("name".into(), s(device_name)),
        ("hostname".into(), s(&pair.hostname)),
        ("setupId".into(), s(setup_id)),
        ("universe".into(), n(universe as i64)),
        ("pairedAt".into(), n(now)),
    ]);
    let put_registry = Put::builder()
        .table_name(&ctx.table)
        .set_item(Some(registry))
        .build()
        .map_err(|e| format!("device registry put malformed: {e}"))?;

    ctx.ddb
        .transact_write_items()
        .transact_items(TransactWriteItem::builder().update(update).build())
        .transact_items(TransactWriteItem::builder().delete(drop_row).build())
        .transact_items(TransactWriteItem::builder().put(put_registry).build())
        .send()
        .await
        .map_err(|e| format!("pair approve failed: {e}"))?;
    Ok(())
}

/// Mark one of the owner's paired devices revoked (soft delete: the row stays
/// for audit, carrying `revoked`/`revokedAt`, and drops out of `/list`). Returns
/// `false` when the caller owns no such device — an idempotent no-op, not an
/// error. Data-plane only; the box's live access is cut at the authorizer in a
/// later phase (docs/claim-code-pairing.md §Revocation).
pub async fn revoke_device(ctx: &Ctx, sub: &str, device_id: &str) -> Result<bool, String> {
    let result = ctx
        .ddb
        .update_item()
        .table_name(&ctx.table)
        .key("pk", AttributeValue::S(device_pk(sub)))
        .key("sk", AttributeValue::S(device_id.to_owned()))
        .update_expression("SET #rv = :true, revokedAt = :now")
        .condition_expression("attribute_exists(pk)")
        .expression_attribute_names("#rv", "revoked")
        .expression_attribute_values(":true", AttributeValue::Bool(true))
        .expression_attribute_values(":now", AttributeValue::N(now_millis().to_string()))
        .send()
        .await;
    match result {
        Ok(_) => Ok(true),
        Err(e) => {
            if e.as_service_error()
                .is_some_and(|se| se.is_conditional_check_failed_exception())
            {
                Ok(false)
            } else {
                Err(format!("device revoke failed: {e}"))
            }
        }
    }
}

/// Claim an approved grant for redemption — the single-use gate. Exactly one
/// caller wins the `approved → redeemed` flip; everyone else hits the
/// condition. (If the mint after this fails, the code is burned and the node
/// simply re-registers — safety over convenience.)
pub async fn redeem_pair(ctx: &Ctx, pair_ref: &str) -> Result<(), String> {
    ctx.ddb
        .update_item()
        .table_name(&ctx.table)
        .key("pk", AttributeValue::S(pair_pk(pair_ref)))
        .key("sk", AttributeValue::S(PAIR_SK.into()))
        .update_expression("SET #st = :redeemed, redeemedAt = :now")
        .condition_expression("#st = :approved")
        .expression_attribute_names("#st", "status")
        .expression_attribute_values(":redeemed", AttributeValue::S("redeemed".into()))
        .expression_attribute_values(":approved", AttributeValue::S("approved".into()))
        .expression_attribute_values(":now", AttributeValue::N(now_millis().to_string()))
        .send()
        .await
        .map_err(|e| format!("pair redeem failed: {e}"))?;
    Ok(())
}
