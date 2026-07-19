//! Shared control — granting one contact the desk for one setup
//! (docs/shared-control.md).
//!
//! Three item families in the `lux-sync` table, all in their own partitions so
//! sync's `USER#<sub>` list query never sees them (the `APPLE#` pattern):
//!
//! - `pk = INVITE#<sha256(code)>,  sk = INVITE` — one outstanding claim code.
//!   Keyed by the hash, so the bearer secret is never at rest; carries `ttl`
//!   and self-expires.
//! - `pk = GRANT#<owner_sub>,      sk = CONTACT#<contact_sub>#SETUP#<setup_id>`
//!   — the owner's manage list. Its `sk = INVITE#<ref>` siblings mirror the
//!   outstanding codes, so the owner can list and withdraw them (and the
//!   per-owner invite cap is one query, not a scan).
//! - `pk = SHARED#<contact_sub>,   sk = OWNER#<owner_sub>#SETUP#<setup_id>`
//!   — the contact's "shared with you" list **and** the IoT authorizer's
//!   connect-time lookup, which is why it is a partition keyed by the
//!   connecting user and not a filter over someone else's.
//!
//! The two halves of a grant are written and deleted in one transaction with
//! `attribute_not_exists` guards: a half-written grant would either be an
//! invisible grant (authorizer allows, owner can't revoke) or an unenforceable
//! one, and neither is allowed to exist. Claiming consumes the invite in that
//! same transaction, which is what makes a code single-use — exactly one
//! concurrent claimer wins the conditional delete.

use std::collections::{HashMap, HashSet};

use aws_sdk_dynamodb::types::{AttributeValue, Delete, Put, TransactWriteItem};
use lambda_http::{Body, Error, Request, Response};
use lux_wire::shares::{
    ClaimRequest, ClaimResponse, Grant, InviteRequest, InviteResponse, PendingInvite,
    ReceivedGrant, RevokeResponse, SharesResponse, INVITE_TTL_SECS, MAX_GRANTS_PER_CONTACT,
    MAX_PENDING_INVITES,
};
use sha2::{Digest, Sha256};

use crate::{error, now_millis, nudge, parse_body, reply, Ctx};

// --- codes ------------------------------------------------------------------

/// Claim-code alphabet: no vowels (so no code ever spells a word) and no
/// 0/O/1/I/L/S lookalikes — the same set the device-pairing display codes use.
/// Note that `L` and `U` are deliberately absent, which is what makes the
/// `LUX` prefix unambiguous to strip on the way back in.
const CODE_ALPHABET: &[u8] = b"23456789CDFGHJKMNPQRTVWXZ";

/// Code body length. 25^10 ≈ 2^46 — a claim is bearer-authorized but reaching
/// it still requires a signed-in account, a live 48-hour window, and surviving
/// single use, so this is a very large multiple of what the exposure needs.
const CODE_LEN: usize = 10;

/// Mint a code in the shape a human sends in a message: `LUX-XXXXX-XXXXX`.
///
/// Rejection-sampled rather than reduced modulo the alphabet, so every symbol
/// is equally likely and the entropy noted above is the real figure rather than
/// an upper bound. (Plain `% 25` would favour 6 of the 25 symbols by about
/// 10%, costing roughly a bit — immaterial against a single-use 48-hour code,
/// but the property is free to keep.)
fn mint_code() -> Result<String, String> {
    let span = CODE_ALPHABET.len() as u16;
    let ceiling = (256 / span) * span; // largest whole multiple of the alphabet
    let mut body = String::with_capacity(CODE_LEN);
    while body.len() < CODE_LEN {
        for b in random::<CODE_LEN>()? {
            if (b as u16) < ceiling {
                body.push(CODE_ALPHABET[b as usize % CODE_ALPHABET.len()] as char);
                if body.len() == CODE_LEN {
                    break;
                }
            }
        }
    }
    Ok(format!("LUX-{}-{}", &body[..5], &body[5..]))
}

/// Undo every formatting kindness a messaging app or a human might apply:
/// case, the grouping dashes, stray whitespace, and the `LUX` prefix.
fn normalize_code(raw: &str) -> String {
    let compact: String = raw
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_uppercase();
    compact.strip_prefix("LUX").unwrap_or(&compact).to_owned()
}

/// The at-rest key for a code: hex sha256 of its normalized body. The code
/// itself never touches the table, so a database reader cannot claim anything.
fn code_ref(code: &str) -> String {
    hex(&Sha256::digest(normalize_code(code).as_bytes()))
}

fn random<const N: usize>() -> Result<[u8; N], String> {
    use std::io::Read;
    let mut bytes = [0u8; N];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| format!("no OS randomness: {e}"))?;
    Ok(bytes)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut acc, b| {
        use std::fmt::Write;
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

// --- keys -------------------------------------------------------------------

fn invite_pk(code_ref: &str) -> String {
    format!("INVITE#{code_ref}")
}

const INVITE_SK: &str = "INVITE";

fn grant_pk(owner_sub: &str) -> String {
    format!("GRANT#{owner_sub}")
}

fn grant_sk(contact_sub: &str, setup_id: &str) -> String {
    format!("CONTACT#{contact_sub}#SETUP#{setup_id}")
}

/// The owner's mirror row for an outstanding invite, in the same partition as
/// their grants so one query serves the whole manage list.
fn pending_sk(code_ref: &str) -> String {
    format!("INVITE#{code_ref}")
}

fn shared_pk(contact_sub: &str) -> String {
    format!("SHARED#{contact_sub}")
}

fn shared_sk(owner_sub: &str, setup_id: &str) -> String {
    format!("OWNER#{owner_sub}#SETUP#{setup_id}")
}

fn s(v: &str) -> AttributeValue {
    AttributeValue::S(v.to_owned())
}

fn n(v: i64) -> AttributeValue {
    AttributeValue::N(v.to_string())
}

fn read_s(item: &HashMap<String, AttributeValue>, key: &str) -> Option<String> {
    item.get(key)?.as_s().ok().cloned()
}

fn read_n(item: &HashMap<String, AttributeValue>, key: &str) -> Option<i64> {
    item.get(key)?.as_n().ok()?.parse().ok()
}

// --- routes -----------------------------------------------------------------

/// `POST /shares/invite` — the owner mints a claim code for one of their setups.
pub async fn invite(
    ctx: &Ctx,
    sub: &str,
    email: Option<&str>,
    req: &Request,
) -> Result<Response<Body>, Error> {
    let body: InviteRequest = match parse_body(req) {
        Ok(b) => b,
        Err(e) => return reply(400, error(&e)),
    };

    // A setup id ends up inside an IAM resource ARN at the authorizer, where
    // `*` matches `/` — so `PUT /setups/*` followed by an invite would widen
    // the grant across the owner's whole setup space. Refuse the id here as
    // well as there; the write path still accepts whatever it always did, so
    // no existing setup stops syncing over this.
    if !is_shareable_id(&body.setup_id) {
        return reply(400, error("that setup cannot be shared"));
    }

    // Only over a setup the caller actually owns and hasn't deleted. This is
    // the authorization check for the whole feature: a grant can only ever name
    // a setup in the minting caller's own partition.
    let setup_name = match live_setup_name(ctx, sub, &body.setup_id).await {
        Ok(Some(name)) => name,
        Ok(None) => return reply(404, error("no such setup")),
        Err(e) => {
            tracing::error!("setup lookup failed: {e}");
            return reply(500, error("internal"));
        }
    };

    let rows = match query_partition(ctx, &grant_pk(sub)).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("invite count failed: {e}");
            return reply(500, error("internal"));
        }
    };
    let now = now_millis();
    let outstanding = rows
        .iter()
        .filter(|item| is_pending_invite(item, now))
        .count();
    if outstanding >= MAX_PENDING_INVITES {
        return reply(
            409,
            error("too many unused invite codes; withdraw one before minting another"),
        );
    }

    let code = match mint_code() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("{e}");
            return reply(500, error("internal"));
        }
    };
    let reference = code_ref(&code);
    let expires_at = now + INVITE_TTL_SECS * 1000;
    // Keep the row a day past expiry so a "why didn't this work" question is
    // still answerable; DynamoDB TTL is epoch seconds.
    let ttl = expires_at / 1000 + 86_400;

    let mut invite_item: HashMap<String, AttributeValue> = HashMap::from([
        ("pk".into(), s(&invite_pk(&reference))),
        ("sk".into(), s(INVITE_SK)),
        ("ownerSub".into(), s(sub)),
        ("ownerLabel".into(), s(&label_for(sub, email))),
        ("setupId".into(), s(&body.setup_id)),
        ("setupName".into(), s(&setup_name)),
        ("createdAt".into(), n(now)),
        ("expiresAt".into(), n(expires_at)),
        ("ttl".into(), n(ttl)),
    ]);
    let mut pending_item: HashMap<String, AttributeValue> = HashMap::from([
        ("pk".into(), s(&grant_pk(sub))),
        ("sk".into(), s(&pending_sk(&reference))),
        ("codeRef".into(), s(&reference)),
        ("setupId".into(), s(&body.setup_id)),
        ("setupName".into(), s(&setup_name)),
        ("createdAt".into(), n(now)),
        ("expiresAt".into(), n(expires_at)),
        ("ttl".into(), n(ttl)),
    ]);
    if let Some(label) = body.label.as_deref().filter(|l| !l.is_empty()) {
        invite_item.insert("label".into(), s(label));
        pending_item.insert("label".into(), s(label));
    }

    if let Err(e) = put_both(ctx, invite_item, pending_item).await {
        tracing::error!("invite write failed: {e}");
        return reply(500, error("internal"));
    }

    // The owner's other devices refresh their manage list.
    nudge(ctx, sub, lux_wire::nudge::shares_changed_frame()).await;

    reply(
        200,
        InviteResponse {
            code,
            code_ref: reference,
            expires_at,
        },
    )
}

/// `POST /shares/claim` — the contact redeems a code and gains the grant.
///
/// Every way a code can fail to be usable — never existed, expired, already
/// claimed, withdrawn — answers identically: a caller learns nothing about
/// codes they don't hold.
pub async fn claim(
    ctx: &Ctx,
    sub: &str,
    email: Option<&str>,
    req: &Request,
) -> Result<Response<Body>, Error> {
    let body: ClaimRequest = match parse_body(req) {
        Ok(b) => b,
        Err(e) => return reply(400, error(&e)),
    };
    let reference = code_ref(&body.code);
    let no_such = || reply(404, error("that invite code is not valid"));

    let invite = match get_item(ctx, &invite_pk(&reference), INVITE_SK).await {
        Ok(Some(item)) => item,
        Ok(None) => return no_such(),
        Err(e) => {
            tracing::error!("invite lookup failed: {e}");
            return reply(500, error("internal"));
        }
    };
    let now = now_millis();
    let (Some(owner_sub), Some(setup_id), Some(expires_at)) = (
        read_s(&invite, "ownerSub"),
        read_s(&invite, "setupId"),
        read_n(&invite, "expiresAt"),
    ) else {
        tracing::error!("malformed invite item");
        return reply(500, error("internal"));
    };
    if expires_at <= now {
        return no_such();
    }
    if owner_sub == sub {
        return reply(400, error("that is your own invite code"));
    }

    // The setup must still be there — an owner who deleted it between minting
    // and claiming would otherwise hand out a grant onto nothing. Re-reading it
    // also refreshes the name the contact will see.
    let setup_name = match live_setup_name(ctx, &owner_sub, &setup_id).await {
        Ok(Some(name)) => name,
        Ok(None) => return reply(409, error("that setup is no longer shared")),
        Err(e) => {
            tracing::error!("setup lookup failed: {e}");
            return reply(500, error("internal"));
        }
    };

    let existing = match query_partition(ctx, &shared_pk(sub)).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("grant count failed: {e}");
            return reply(500, error("internal"));
        }
    };
    if existing
        .iter()
        .any(|item| read_s(item, "sk").as_deref() == Some(&shared_sk(&owner_sub, &setup_id)))
    {
        return reply(409, error("you already have access to that setup"));
    }
    // The cap the IoT authorizer's policy-document budget forces (see
    // lux_wire::shares::MAX_GRANTS_PER_CONTACT). Refusing here is the loud half
    // of "cap loudly"; the authorizer truncating is the silent half we never
    // want to reach. Simultaneous claims can all pass this read before any of
    // them commits, so the overshoot is bounded by how many live codes one
    // contact holds at once, not by one — the authorizer is the enforcing line
    // either way, and it fails closed by truncating.
    if existing.len() >= MAX_GRANTS_PER_CONTACT {
        return reply(
            409,
            error("you are already in as many shared setups as one account can hold"),
        );
    }

    let owner_label = read_s(&invite, "ownerLabel").unwrap_or_default();
    let contact_label = label_for(sub, email);
    let label = read_s(&invite, "label");

    let mut grant_item: HashMap<String, AttributeValue> = HashMap::from([
        ("pk".into(), s(&grant_pk(&owner_sub))),
        ("sk".into(), s(&grant_sk(sub, &setup_id))),
        ("contactSub".into(), s(sub)),
        ("contactLabel".into(), s(&contact_label)),
        ("setupId".into(), s(&setup_id)),
        ("setupName".into(), s(&setup_name)),
        ("createdAt".into(), n(now)),
    ]);
    if let Some(label) = label.as_deref() {
        grant_item.insert("label".into(), s(label));
    }
    let shared_item: HashMap<String, AttributeValue> = HashMap::from([
        ("pk".into(), s(&shared_pk(sub))),
        ("sk".into(), s(&shared_sk(&owner_sub, &setup_id))),
        ("ownerSub".into(), s(&owner_sub)),
        ("ownerLabel".into(), s(&owner_label)),
        ("setupId".into(), s(&setup_id)),
        ("setupName".into(), s(&setup_name)),
        ("createdAt".into(), n(now)),
    ]);

    // One transaction: burn the code (both its rows) and write both halves of
    // the grant. The conditional delete is the single-use gate.
    let burn = Delete::builder()
        .table_name(&ctx.table)
        .key("pk", s(&invite_pk(&reference)))
        .key("sk", s(INVITE_SK))
        .condition_expression("attribute_exists(pk) AND expiresAt > :now")
        .expression_attribute_values(":now", n(now))
        .build()
        .map_err(|e| format!("invite delete malformed: {e}"))?;
    let drop_pending = Delete::builder()
        .table_name(&ctx.table)
        .key("pk", s(&grant_pk(&owner_sub)))
        .key("sk", s(&pending_sk(&reference)))
        .build()
        .map_err(|e| format!("pending delete malformed: {e}"))?;
    let put = |item| {
        Put::builder()
            .table_name(&ctx.table)
            .set_item(Some(item))
            .condition_expression("attribute_not_exists(pk)")
            .build()
            .map_err(|e| format!("grant put malformed: {e}"))
    };
    if let Err(e) = ctx
        .ddb
        .transact_write_items()
        .transact_items(TransactWriteItem::builder().delete(burn).build())
        .transact_items(TransactWriteItem::builder().delete(drop_pending).build())
        .transact_items(TransactWriteItem::builder().put(put(grant_item)?).build())
        .transact_items(TransactWriteItem::builder().put(put(shared_item)?).build())
        .send()
        .await
    {
        // Losing the race for a single-use code is the expected shape of this
        // failure, so it reads as an invalid code rather than an error.
        tracing::warn!("claim transaction rejected: {e}");
        return no_such();
    }

    // Both parties learn immediately: the owner's manage list gains a contact,
    // the contact's shared list gains a setup.
    nudge(ctx, &owner_sub, lux_wire::nudge::shares_changed_frame()).await;
    nudge(ctx, sub, lux_wire::nudge::shares_changed_frame()).await;

    reply(
        200,
        ClaimResponse {
            owner_sub,
            owner_label,
            setup_id,
            setup_name: Some(setup_name),
        },
    )
}

/// `GET /shares` — both directions plus the caller's outstanding invites.
pub async fn list(ctx: &Ctx, sub: &str) -> Result<Response<Body>, Error> {
    let (owned_pk, received_pk) = (grant_pk(sub), shared_pk(sub));
    let (owned, received) = match tokio::try_join!(
        query_partition(ctx, &owned_pk),
        query_partition(ctx, &received_pk),
    ) {
        Ok(pair) => pair,
        Err(e) => {
            tracing::error!("shares list failed: {e}");
            return reply(500, error("internal"));
        }
    };

    let now = now_millis();
    let mut granted = Vec::new();
    let mut pending = Vec::new();
    for item in &owned {
        let Some(sk) = read_s(item, "sk") else { continue };
        if sk.starts_with("CONTACT#") {
            let (Some(contact_sub), Some(setup_id), Some(created_at)) = (
                read_s(item, "contactSub"),
                read_s(item, "setupId"),
                read_n(item, "createdAt"),
            ) else {
                continue;
            };
            granted.push(Grant {
                contact_sub,
                contact_label: read_s(item, "contactLabel").unwrap_or_default(),
                setup_id,
                setup_name: read_s(item, "setupName"),
                label: read_s(item, "label"),
                created_at,
            });
        } else if is_pending_invite(item, now) {
            let (Some(code_ref), Some(setup_id), Some(created_at), Some(expires_at)) = (
                read_s(item, "codeRef"),
                read_s(item, "setupId"),
                read_n(item, "createdAt"),
                read_n(item, "expiresAt"),
            ) else {
                continue;
            };
            pending.push(PendingInvite {
                code_ref,
                setup_id,
                setup_name: read_s(item, "setupName"),
                label: read_s(item, "label"),
                created_at,
                expires_at,
            });
        }
    }

    let received = received
        .iter()
        .filter_map(|item| {
            Some(ReceivedGrant {
                owner_sub: read_s(item, "ownerSub")?,
                owner_label: read_s(item, "ownerLabel").unwrap_or_default(),
                setup_id: read_s(item, "setupId")?,
                setup_name: read_s(item, "setupName"),
                created_at: read_n(item, "createdAt")?,
            })
        })
        .collect();

    reply(
        200,
        SharesResponse {
            granted,
            received,
            pending,
        },
    )
}

/// `DELETE /shares/granted/{contactSub}/{setupId}` (owner revokes) and
/// `DELETE /shares/received/{ownerSub}/{setupId}` (contact leaves).
///
/// Both name the same grant from opposite ends and do the same thing, which is
/// the point: leaving is not a lesser operation than revoking, and neither side
/// needs the other's cooperation to end the arrangement.
pub async fn revoke(
    ctx: &Ctx,
    owner_sub: &str,
    contact_sub: &str,
    setup_id: &str,
    caller_is_owner: bool,
) -> Result<Response<Body>, Error> {
    match delete_grant(ctx, owner_sub, contact_sub, setup_id, caller_is_owner).await {
        Ok(()) => {}
        Err(e) => {
            // The caller's own half not existing is the only ordinary failure
            // here, and it means there is nothing to revoke.
            tracing::warn!("grant delete rejected: {e}");
            return reply(404, error("no such share"));
        }
    }

    nudge(ctx, owner_sub, lux_wire::nudge::shares_changed_frame()).await;
    nudge(ctx, contact_sub, lux_wire::nudge::shares_changed_frame()).await;

    reply(200, RevokeResponse { revoked: true })
}

/// `DELETE /shares/invite/{codeRef}` — the owner withdraws an unclaimed code.
pub async fn withdraw(ctx: &Ctx, sub: &str, reference: &str) -> Result<Response<Body>, Error> {
    // The ownership condition is on the invite row, which is the one an
    // attacker could name: a `codeRef` from someone else's pending list is
    // useless without also being that owner.
    let burn = Delete::builder()
        .table_name(&ctx.table)
        .key("pk", s(&invite_pk(reference)))
        .key("sk", s(INVITE_SK))
        .condition_expression("ownerSub = :sub")
        .expression_attribute_values(":sub", s(sub))
        .build()
        .map_err(|e| format!("invite delete malformed: {e}"))?;
    let drop_pending = Delete::builder()
        .table_name(&ctx.table)
        .key("pk", s(&grant_pk(sub)))
        .key("sk", s(&pending_sk(reference)))
        .build()
        .map_err(|e| format!("pending delete malformed: {e}"))?;

    if let Err(e) = ctx
        .ddb
        .transact_write_items()
        .transact_items(TransactWriteItem::builder().delete(burn).build())
        .transact_items(TransactWriteItem::builder().delete(drop_pending).build())
        .send()
        .await
    {
        tracing::warn!("invite withdraw rejected: {e}");
        return reply(404, error("no such invite"));
    }

    nudge(ctx, sub, lux_wire::nudge::shares_changed_frame()).await;
    reply(200, RevokeResponse { revoked: true })
}

// --- account deletion -------------------------------------------------------

/// Everything shared control leaves behind, cleared before the caller's own
/// partition is wiped (`DELETE /user`).
///
/// This runs in both directions and neither is optional. As an **owner**, the
/// caller's grants live half in their own partition and half in each contact's;
/// wiping only their own would leave every contact holding a "shared with you"
/// row forever, pointing at an account that no longer exists. As a **contact**,
/// the mirror sits in each owner's partition and would show a ghost in their
/// manage list with a revoke button that does nothing.
///
/// The retained `config` topics go too: they carry the setup's name and channel
/// labels, and deleting an account has to mean the data actually leaves.
///
/// **Fallible on purpose, and the caller must not wipe on an error.** An
/// earlier draft of this ran best-effort and claimed a stale half "resolves as
/// no access" — that is false, and the module header above says why: the
/// authorizer reads `SHARED#<sub>` and nothing else, so a surviving mirror is a
/// live grant into a topic space whose owner no longer exists, with both the
/// owner's row and the owner's Cognito user gone. There is no revoking that.
///
/// So a read or delete that fails here fails the whole request. Deletion is
/// already idempotent and retryable end to end (the app deletes the Cognito
/// user only after this returns), and a retry re-reads a shorter list and
/// finishes the job.
pub async fn cleanup_for_deleted_user(ctx: &Ctx, sub: &str) -> Result<(), String> {
    let owned = query_partition(ctx, &grant_pk(sub)).await?;
    let received = query_partition(ctx, &shared_pk(sub)).await?;

    let mut notify: HashSet<String> = HashSet::new();

    // As owner: drop each contact's mirror.
    for item in &owned {
        let Some(sk) = read_s(item, "sk") else { continue };
        if let Some(contact_sub) = read_s(item, "contactSub") {
            if sk.starts_with("CONTACT#") {
                delete_key(ctx, &shared_pk(&contact_sub), &mirror_sk_of_owned(item, sub)).await?;
                notify.insert(contact_sub);
            }
        }
        // Outstanding invites: the code row lives in its own partition and
        // would otherwise stay claimable until its TTL.
        if let Some(reference) = read_s(item, "codeRef") {
            delete_key(ctx, &invite_pk(&reference), INVITE_SK).await?;
        }
        delete_key(ctx, &grant_pk(sub), &sk).await?;
    }

    // As contact: drop each owner's half of the grant.
    for item in &received {
        let (Some(sk), Some(owner_sub), Some(setup_id)) = (
            read_s(item, "sk"),
            read_s(item, "ownerSub"),
            read_s(item, "setupId"),
        ) else {
            continue;
        };
        delete_key(ctx, &grant_pk(&owner_sub), &grant_sk(sub, &setup_id)).await?;
        delete_key(ctx, &shared_pk(sub), &sk).await?;
        notify.insert(owner_sub);
    }

    // Retained config carries the setup's name and every channel label. Sweep
    // *every* setup the account has, not just the currently-granted ones: a
    // setup that was shared and then revoked still has a retained config, and
    // driving this from live grants alone would walk straight past it.
    // Clearing a topic that never had a retained message is a no-op.
    for item in query_partition(ctx, &format!("USER#{sub}")).await? {
        if let Some(setup_id) = read_s(&item, "sk").and_then(|sk| {
            sk.strip_prefix("SETUP#")
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
        }) {
            clear_retained_config(ctx, sub, &setup_id).await;
        }
    }

    // Nudges stay best-effort, as everywhere else: a missed one is healed by
    // the other party's next pull, and it is not a correctness boundary.
    for other in &notify {
        nudge(ctx, other, lux_wire::nudge::shares_changed_frame()).await;
    }
    Ok(())
}

/// Given a row from the owner's `GRANT#` partition, the key of its mirror in
/// the contact's `SHARED#` partition. The two sort keys are not symmetrical —
/// each names the *other* party — so deleting the far half means rebuilding its
/// key rather than reusing the near one.
fn mirror_sk_of_owned(item: &HashMap<String, AttributeValue>, owner_sub: &str) -> String {
    let setup_id = read_s(item, "setupId").unwrap_or_default();
    shared_sk(owner_sub, &setup_id)
}

/// Publish an empty retained payload to a setup's config topic, deleting the
/// retained message. Best-effort like every other publish on this path.
async fn clear_retained_config(ctx: &Ctx, sub: &str, setup_id: &str) {
    let Some(iot) = &ctx.iot else { return };
    let topic = lux_wire::ctl::config_topic(sub, setup_id);
    if let Err(e) = iot
        .publish()
        .topic(&topic)
        .qos(0)
        .retain(true)
        .payload(aws_sdk_iotdataplane::primitives::Blob::new(Vec::new()))
        .send()
        .await
    {
        tracing::warn!("retained config clear on {topic} failed: {e}");
    }
}

// --- store helpers ----------------------------------------------------------

/// The name of a live (existing, untombstoned) setup in a user's partition.
async fn live_setup_name(ctx: &Ctx, sub: &str, setup_id: &str) -> Result<Option<String>, String> {
    let out = ctx
        .ddb
        .get_item()
        .table_name(&ctx.table)
        .key("pk", s(&format!("USER#{sub}")))
        .key("sk", s(&format!("SETUP#{setup_id}")))
        .send()
        .await
        .map_err(|e| format!("setup get failed: {e}"))?;
    let Some(item) = out.item else {
        return Ok(None);
    };
    let deleted = item
        .get("deleted")
        .and_then(|v| v.as_bool().ok().copied())
        .unwrap_or(false);
    if deleted {
        return Ok(None);
    }
    Ok(Some(read_s(&item, "name").unwrap_or_default()))
}

async fn get_item(
    ctx: &Ctx,
    pk: &str,
    sk: &str,
) -> Result<Option<HashMap<String, AttributeValue>>, String> {
    let out = ctx
        .ddb
        .get_item()
        .table_name(&ctx.table)
        .key("pk", s(pk))
        .key("sk", s(sk))
        .send()
        .await
        .map_err(|e| format!("get failed: {e}"))?;
    Ok(out.item)
}

/// Every item in one partition, paged. These partitions hold at most a handful
/// of rows each (both caps are single digits), so paging is belt-and-braces.
async fn query_partition(
    ctx: &Ctx,
    pk: &str,
) -> Result<Vec<HashMap<String, AttributeValue>>, String> {
    let mut items = Vec::new();
    let mut start_key = None;
    loop {
        let out = ctx
            .ddb
            .query()
            .table_name(&ctx.table)
            .key_condition_expression("pk = :pk")
            .expression_attribute_values(":pk", s(pk))
            .set_exclusive_start_key(start_key.take())
            .send()
            .await
            .map_err(|e| format!("query failed: {e}"))?;
        items.extend(out.items.clone().unwrap_or_default());
        start_key = out.last_evaluated_key().cloned();
        if start_key.is_none() {
            return Ok(items);
        }
    }
}

/// Write two items in one transaction, each insisting it is new.
async fn put_both(
    ctx: &Ctx,
    first: HashMap<String, AttributeValue>,
    second: HashMap<String, AttributeValue>,
) -> Result<(), String> {
    let put = |item| {
        Put::builder()
            .table_name(&ctx.table)
            .set_item(Some(item))
            .condition_expression("attribute_not_exists(pk)")
            .build()
            .map_err(|e| format!("put malformed: {e}"))
    };
    ctx.ddb
        .transact_write_items()
        .transact_items(TransactWriteItem::builder().put(put(first)?).build())
        .transact_items(TransactWriteItem::builder().put(put(second)?).build())
        .send()
        .await
        .map_err(|e| format!("transaction failed: {e}"))?;
    Ok(())
}

/// Drop both halves of a grant in one transaction.
///
/// The existence condition goes on the *caller's* half, so a revoke of nothing
/// is an error rather than a silent success — and so either party can always
/// clear their own row even if the other half has somehow gone missing. Putting
/// it unconditionally on the owner's half would let a stray `SHARED#` row trap
/// a contact in a share they cannot leave.
async fn delete_grant(
    ctx: &Ctx,
    owner_sub: &str,
    contact_sub: &str,
    setup_id: &str,
    caller_is_owner: bool,
) -> Result<(), String> {
    let exists = "attribute_exists(pk)";
    let mut owner_half = Delete::builder()
        .table_name(&ctx.table)
        .key("pk", s(&grant_pk(owner_sub)))
        .key("sk", s(&grant_sk(contact_sub, setup_id)));
    let mut contact_half = Delete::builder()
        .table_name(&ctx.table)
        .key("pk", s(&shared_pk(contact_sub)))
        .key("sk", s(&shared_sk(owner_sub, setup_id)));
    if caller_is_owner {
        owner_half = owner_half.condition_expression(exists);
    } else {
        contact_half = contact_half.condition_expression(exists);
    }
    let owner_half = owner_half
        .build()
        .map_err(|e| format!("grant delete malformed: {e}"))?;
    let contact_half = contact_half
        .build()
        .map_err(|e| format!("mirror delete malformed: {e}"))?;
    ctx.ddb
        .transact_write_items()
        .transact_items(TransactWriteItem::builder().delete(owner_half).build())
        .transact_items(TransactWriteItem::builder().delete(contact_half).build())
        .send()
        .await
        .map_err(|e| format!("grant delete failed: {e}"))?;
    Ok(())
}

/// Single-item delete for the cleanup paths. Failing loudly matters here: a
/// mirror that survives is access nobody can revoke (see
/// [`cleanup_for_deleted_user`]).
async fn delete_key(ctx: &Ctx, pk: &str, sk: &str) -> Result<(), String> {
    ctx.ddb
        .delete_item()
        .table_name(&ctx.table)
        .key("pk", s(pk))
        .key("sk", s(sk))
        .send()
        .await
        .map_err(|e| format!("cleanup delete of {pk}/{sk} failed: {e}"))?;
    Ok(())
}

/// Can this setup id safely become part of an IAM resource ARN? Setup ids are
/// UUIDs in practice, but the write path never enforced that, and "in practice"
/// is not a check when the answer decides how wide a grant is.
fn is_shareable_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Is this row an outstanding (unclaimed, unexpired) invite?
fn is_pending_invite(item: &HashMap<String, AttributeValue>, now: i64) -> bool {
    read_s(item, "sk").is_some_and(|sk| sk.starts_with("INVITE#"))
        && read_n(item, "expiresAt").is_some_and(|exp| exp > now)
}

/// How the caller should appear in the other party's list. The email claim is
/// display-only and optional (an appliance session's token has none), so a
/// missing one degrades to an unlabelled share rather than blocking it — the
/// grant is authorized by `sub`, which is always present.
fn label_for(sub: &str, email: Option<&str>) -> String {
    match email.filter(|e| !e.is_empty()) {
        Some(email) => email.to_owned(),
        None => {
            tracing::info!("no email claim for {sub}; sharing with an unlabelled account");
            String::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minted_codes_use_the_display_alphabet() {
        let code = mint_code().unwrap();
        assert!(code.starts_with("LUX-"));
        let body: String = code.chars().filter(char::is_ascii_alphanumeric).collect();
        assert_eq!(body.len(), "LUX".len() + CODE_LEN);
        assert!(body["LUX".len()..]
            .bytes()
            .all(|b| CODE_ALPHABET.contains(&b)));
    }

    #[test]
    fn normalization_survives_however_a_human_sends_it() {
        let canonical = normalize_code("LUX-4KPT9-XQ2WM");
        assert_eq!(canonical, "4KPT9XQ2WM");
        // Lowercased by a keyboard, re-spaced by a chat app, pasted bare.
        assert_eq!(normalize_code("lux-4kpt9-xq2wm"), canonical);
        assert_eq!(normalize_code("  LUX 4KPT9 XQ2WM "), canonical);
        assert_eq!(normalize_code("4KPT9XQ2WM"), canonical);
        // The prefix is unambiguous because L and U are not in the alphabet, so
        // stripping it can never eat a code's own leading characters.
        assert!(!CODE_ALPHABET.contains(&b'L') && !CODE_ALPHABET.contains(&b'U'));
    }

    #[test]
    fn code_ref_is_a_stable_hash_not_the_secret() {
        let code = "LUX-4KPT9-XQ2WM";
        let reference = code_ref(code);
        assert_eq!(reference.len(), 64);
        assert_eq!(reference, code_ref(code));
        // Every spelling of one code resolves to one row.
        assert_eq!(reference, code_ref("lux 4kpt9 xq2wm"));
        assert!(!reference.contains("4KPT9"));
        assert_ne!(reference, code_ref("LUX-4KPT9-XQ2WN"));
    }

    #[test]
    fn keys_keep_the_two_halves_addressable_from_either_end() {
        // The owner's half is found by (owner, contact, setup); the contact's
        // by (contact, owner, setup). Revoke and leave each hold one and derive
        // the other, so both must be pure functions of the same triple.
        assert_eq!(grant_pk("owner-1"), "GRANT#owner-1");
        assert_eq!(grant_sk("contact-1", "s-1"), "CONTACT#contact-1#SETUP#s-1");
        assert_eq!(shared_pk("contact-1"), "SHARED#contact-1");
        assert_eq!(shared_sk("owner-1", "s-1"), "OWNER#owner-1#SETUP#s-1");
        // Pending invites share the owner's partition, distinguished by prefix.
        assert!(pending_sk("abc").starts_with("INVITE#"));
        assert!(!grant_sk("c", "s").starts_with("INVITE#"));
    }

    #[test]
    fn deletion_rebuilds_the_far_half_of_a_grant() {
        // An owner-side row names the contact; its mirror names the owner. The
        // deletion path holds the former and must derive the latter, and
        // getting this wrong would silently leave every contact holding a row
        // pointing at a deleted account.
        let owned = HashMap::from([
            ("sk".to_owned(), s(&grant_sk("contact-1", "s-1"))),
            ("contactSub".to_owned(), s("contact-1")),
            ("setupId".to_owned(), s("s-1")),
        ]);
        assert_eq!(
            mirror_sk_of_owned(&owned, "owner-1"),
            shared_sk("owner-1", "s-1")
        );
        // …and the round trip closes: the mirror's own key rebuilds the near
        // half, which is what the contact-side deletion loop does.
        assert_eq!(Some(grant_sk("contact-1", "s-1")), read_s(&owned, "sk"));
    }

    #[test]
    fn ids_that_would_widen_a_grant_are_not_shareable() {
        // A UUID, which is what every real setup id is.
        assert!(is_shareable_id("7f0175a6-3b64-4a2a-9e1c-000000000001"));
        // `PUT /setups/*` is a legal request, so this id can genuinely exist —
        // and in an ARN it would grant the owner's entire setup space, because
        // IAM wildcards match `/`.
        assert!(!is_shareable_id("*"));
        assert!(!is_shareable_id("a/b"));
        assert!(!is_shareable_id("s-1/../s-2"));
        assert!(!is_shareable_id(""));
        assert!(!is_shareable_id(&"x".repeat(65)));
    }

    #[test]
    fn pending_invites_are_the_unexpired_invite_rows() {
        let row = |sk: &str, expires: i64| {
            HashMap::from([("sk".to_owned(), s(sk)), ("expiresAt".to_owned(), n(expires))])
        };
        let now = 1_000;
        assert!(is_pending_invite(&row("INVITE#abc", now + 1), now));
        // Expired codes stay in the table until TTL sweeps them, but they are
        // not offered to the owner and never count against the cap.
        assert!(!is_pending_invite(&row("INVITE#abc", now), now));
        assert!(!is_pending_invite(
            &row("CONTACT#c#SETUP#s", now + 1),
            now
        ));
    }
}
