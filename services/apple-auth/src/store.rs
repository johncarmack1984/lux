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
