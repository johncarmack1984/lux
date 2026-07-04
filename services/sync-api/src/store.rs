//! DynamoDB persistence for a user's setups.
//!
//! One item per setup: `pk = USER#<sub>`, `sk = SETUP#<setupId>`. `updatedAt`
//! (epoch millis, assigned here on the server — never the client clock) is the
//! last-writer-wins authority and the optimistic-concurrency token; writes are
//! conditional on it. Fixtures are stored as an opaque JSON string so this layer
//! stays agnostic to the app's fixture schema. Deletes are soft tombstones
//! (`deleted = true`) so other devices learn of them on their next pull.

use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::types::{AttributeValue, ReturnValue};
use aws_sdk_dynamodb::Client;
use lux_wire::{SetupRecord, UpsertSetupBody};
use serde_json::Value;
use std::collections::HashMap;

/// The result of a successful write: the new server timestamp + revision.
#[derive(Debug)]
pub struct WriteResult {
    pub updated_at: i64,
    pub rev: i64,
}

#[derive(Debug)]
pub enum StoreError {
    /// The conditional write failed — another device wrote first.
    Conflict,
    Internal(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Conflict => write!(f, "conflict"),
            StoreError::Internal(e) => write!(f, "{e}"),
        }
    }
}

fn pk(sub: &str) -> String {
    format!("USER#{sub}")
}

fn sk(setup_id: &str) -> String {
    format!("SETUP#{setup_id}")
}

/// All of a user's setups, including tombstones (the client filters them).
pub async fn list(ddb: &Client, table: &str, sub: &str) -> Result<Vec<SetupRecord>, StoreError> {
    let out = ddb
        .query()
        .table_name(table)
        .key_condition_expression("pk = :pk")
        .expression_attribute_values(":pk", AttributeValue::S(pk(sub)))
        .send()
        .await
        .map_err(internal)?;

    let mut setups = Vec::new();
    for item in out.items() {
        // Only setup items; skip any per-user META row that may share the pk.
        let Some(sk_val) = s(item, "sk") else {
            continue;
        };
        let Some(id) = sk_val.strip_prefix("SETUP#") else {
            continue;
        };
        setups.push(SetupRecord {
            id: id.to_owned(),
            name: s(item, "name").unwrap_or_default(),
            universe: n(item, "universe").unwrap_or(1) as u16,
            fixtures: s(item, "fixtures")
                .and_then(|raw| serde_json::from_str(&raw).ok())
                .unwrap_or(Value::Array(vec![])),
            rev: n(item, "rev").unwrap_or(0),
            updated_at: n(item, "updatedAt").unwrap_or(0),
            deleted: item
                .get("deleted")
                .and_then(|v| v.as_bool().ok().copied())
                .unwrap_or(false),
        });
    }
    Ok(setups)
}

/// Create or update one setup with optimistic concurrency. With `base_updated_at`
/// set, the write only lands if the stored `updatedAt` still matches it; without
/// it, the write only lands if the item does not yet exist.
pub async fn upsert(
    ddb: &Client,
    table: &str,
    sub: &str,
    setup_id: &str,
    body: &UpsertSetupBody,
    now: i64,
) -> Result<WriteResult, StoreError> {
    let mut req = ddb
        .update_item()
        .table_name(table)
        .key("pk", AttributeValue::S(pk(sub)))
        .key("sk", AttributeValue::S(sk(setup_id)))
        .update_expression(
            "SET #name = :name, #universe = :universe, #fixtures = :fixtures, \
             #deleted = :false, #updatedAt = :now, #rev = if_not_exists(#rev, :zero) + :one",
        )
        .expression_attribute_names("#name", "name")
        .expression_attribute_names("#universe", "universe")
        .expression_attribute_names("#fixtures", "fixtures")
        .expression_attribute_names("#deleted", "deleted")
        .expression_attribute_names("#updatedAt", "updatedAt")
        .expression_attribute_names("#rev", "rev")
        .expression_attribute_values(":name", AttributeValue::S(body.name.clone()))
        .expression_attribute_values(":universe", AttributeValue::N(body.universe.to_string()))
        .expression_attribute_values(":fixtures", AttributeValue::S(body.fixtures.to_string()))
        .expression_attribute_values(":false", AttributeValue::Bool(false))
        .expression_attribute_values(":now", AttributeValue::N(now.to_string()))
        .expression_attribute_values(":zero", AttributeValue::N("0".into()))
        .expression_attribute_values(":one", AttributeValue::N("1".into()))
        .return_values(ReturnValue::AllNew);

    req = match body.base_updated_at {
        Some(base) => req
            .condition_expression("#updatedAt = :base")
            .expression_attribute_values(":base", AttributeValue::N(base.to_string())),
        None => req.condition_expression("attribute_not_exists(pk)"),
    };

    let out = req.send().await.map_err(conflict_or_internal)?;
    let attrs = out
        .attributes()
        .ok_or_else(|| StoreError::Internal("no attributes returned".into()))?;
    Ok(WriteResult {
        updated_at: n(attrs, "updatedAt").unwrap_or(now),
        rev: n(attrs, "rev").unwrap_or(0),
    })
}

/// Soft-delete a setup (write a tombstone) with optimistic concurrency.
pub async fn tombstone(
    ddb: &Client,
    table: &str,
    sub: &str,
    setup_id: &str,
    base_updated_at: Option<i64>,
    now: i64,
) -> Result<i64, StoreError> {
    let mut req = ddb
        .update_item()
        .table_name(table)
        .key("pk", AttributeValue::S(pk(sub)))
        .key("sk", AttributeValue::S(sk(setup_id)))
        .update_expression(
            "SET #deleted = :true, #updatedAt = :now, #rev = if_not_exists(#rev, :zero) + :one",
        )
        .expression_attribute_names("#deleted", "deleted")
        .expression_attribute_names("#updatedAt", "updatedAt")
        .expression_attribute_names("#rev", "rev")
        .expression_attribute_values(":true", AttributeValue::Bool(true))
        .expression_attribute_values(":now", AttributeValue::N(now.to_string()))
        .expression_attribute_values(":zero", AttributeValue::N("0".into()))
        .expression_attribute_values(":one", AttributeValue::N("1".into()))
        .return_values(ReturnValue::AllNew);

    req = match base_updated_at {
        Some(base) => req
            .condition_expression("#updatedAt = :base")
            .expression_attribute_values(":base", AttributeValue::N(base.to_string())),
        None => req.condition_expression("attribute_exists(pk)"),
    };

    let out = req.send().await.map_err(conflict_or_internal)?;
    let attrs = out
        .attributes()
        .ok_or_else(|| StoreError::Internal("no attributes returned".into()))?;
    Ok(n(attrs, "updatedAt").unwrap_or(now))
}

/// Hard-delete every item in a user's partition (account deletion — the data
/// must actually leave the table, so no tombstones). Pages through the
/// partition and deletes item-by-item; a user's partition is small (their
/// setups), and per-item deletes keep the required IAM surface minimal.
/// Idempotent: a retry after a partial failure deletes whatever remains.
pub async fn delete_all(ddb: &Client, table: &str, sub: &str) -> Result<i64, StoreError> {
    let mut deleted = 0i64;
    let mut start_key: Option<HashMap<String, AttributeValue>> = None;
    loop {
        let out = ddb
            .query()
            .table_name(table)
            .key_condition_expression("pk = :pk")
            .expression_attribute_values(":pk", AttributeValue::S(pk(sub)))
            .projection_expression("pk, sk")
            .set_exclusive_start_key(start_key.take())
            .send()
            .await
            .map_err(internal)?;
        for item in out.items() {
            let (Some(pk_val), Some(sk_val)) = (item.get("pk"), item.get("sk")) else {
                continue;
            };
            ddb.delete_item()
                .table_name(table)
                .key("pk", pk_val.clone())
                .key("sk", sk_val.clone())
                .send()
                .await
                .map_err(internal)?;
            deleted += 1;
        }
        start_key = out.last_evaluated_key().cloned();
        if start_key.is_none() {
            return Ok(deleted);
        }
    }
}

// --- helpers ----------------------------------------------------------------

fn s(item: &HashMap<String, AttributeValue>, key: &str) -> Option<String> {
    item.get(key)?.as_s().ok().cloned()
}

fn n(item: &HashMap<String, AttributeValue>, key: &str) -> Option<i64> {
    item.get(key)?.as_n().ok()?.parse().ok()
}

fn internal<E: std::fmt::Display>(e: E) -> StoreError {
    StoreError::Internal(e.to_string())
}

/// Map a conditional-check failure to [`StoreError::Conflict`], anything else to
/// [`StoreError::Internal`].
fn conflict_or_internal<E, R>(err: SdkError<E, R>) -> StoreError
where
    E: ConditionalCheck + std::fmt::Display,
{
    if err
        .as_service_error()
        .is_some_and(|e| e.is_conditional_check())
    {
        StoreError::Conflict
    } else {
        StoreError::Internal(err.to_string())
    }
}

/// Lets [`conflict_or_internal`] ask any DynamoDB write error whether it was a
/// conditional-check failure, without naming each operation's error type.
trait ConditionalCheck {
    fn is_conditional_check(&self) -> bool;
}

impl ConditionalCheck for aws_sdk_dynamodb::operation::update_item::UpdateItemError {
    fn is_conditional_check(&self) -> bool {
        self.is_conditional_check_failed_exception()
    }
}
