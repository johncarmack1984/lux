//! The six routes, in the order a church meets them.
//!
//! `connect` → the admin approves at Planning Center → `callback` lands the
//! tokens → `status` says it took → `service-types` picks which service the
//! setup follows → `plan` reads this Sunday. `disconnect` undoes all of it.
//!
//! Two rules hold everywhere below:
//!
//! - **Identity is the verified bearer's `sub`, never a request field.** A
//!   church can only ever read the connection it authorized.
//! - **Planning Center's failures are translated, not forwarded.** A 401 from
//!   them becomes "reconnect", a 429 becomes "busy, try again", and neither
//!   carries their body through to a surface.

use std::sync::Arc;

use lux_cue::CueSheet;
use lux_pco::{Error as PcoError, PcoClient};
use lux_wire::plan::{
    ConnectResponse, PlanItemSummary, PlanRequest, PlanResponse, PlanSummary, ServiceTypeSummary,
    ServiceTypesResponse, StatusResponse,
};
use serde_json::Value;

use crate::http::{body_bytes, caller, error, html, reply, UrlEvent};
use crate::store::{self, Connection};
use crate::tokens;
use crate::Ctx;

/// How long the church's administrator has to finish at Planning Center.
const STATE_TTL_SECS: i64 = 900;

/// `POST /pco/connect` — mint the authorize URL for this account.
pub async fn connect(ctx: &Arc<Ctx>, event: &UrlEvent) -> Result<Value, Error> {
    let Some(sub) = caller(ctx, event) else {
        return reply(401, &error("unauthorized"));
    };
    let app = match ctx.oauth().await {
        Ok(a) => a,
        Err(e) => {
            tracing::error!("oauth app unavailable: {e}");
            return reply(503, &error("planning center is not configured"));
        }
    };

    let state = match rand_token() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("connect: no OS randomness: {e}");
            return reply(500, &error("internal"));
        }
    };
    if let Err(e) = store::put_state(ctx, &state, &sub, STATE_TTL_SECS).await {
        tracing::error!("connect state write failed: {e}");
        return reply(500, &error("internal"));
    }

    reply(
        200,
        &ConnectResponse {
            authorize_url: app.authorize_url(&state),
            state,
        },
    )
}

/// `GET /pco/callback?code=…&state=…` — Planning Center's redirect.
///
/// Always answers with a page a human can read, never JSON and never a
/// redirect: the browser that lands here may be on a different machine from
/// the app, so "go back to lux" is an instruction, not a navigation.
pub async fn callback(ctx: &Arc<Ctx>, event: &UrlEvent) -> Result<Value, Error> {
    // Planning Center's own refusal (the admin pressed Deny) arrives as
    // `error`, with no code. Say so plainly rather than showing a failure.
    if let Some(denied) = event.query("error") {
        tracing::info!("pco callback returned an error: {denied}");
        return page(
            400,
            "Connection cancelled",
            "Planning Center did not approve the connection. You can close this window and try again from lux.",
        );
    }

    let (Some(code), Some(state)) = (event.query("code"), event.query("state")) else {
        return page(
            400,
            "Something went missing",
            "That link is incomplete. Start the connection again from lux.",
        );
    };

    // Single-use: a replayed callback finds nothing here.
    let sub = match store::take_state(ctx, state).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return page(
                400,
                "That link has expired",
                "Connection links are good for fifteen minutes and can only be used once. Start again from lux.",
            );
        }
        Err(e) => {
            tracing::error!("callback state take failed: {e}");
            return page(500, "Something went wrong", "Please try again from lux.");
        }
    };

    let app = match ctx.oauth().await {
        Ok(a) => a,
        Err(e) => {
            tracing::error!("oauth app unavailable: {e}");
            return page(503, "Not configured yet", "Please try again later.");
        }
    };
    let tokens = match app.exchange_code(&ctx.http, code).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("pco code exchange failed: {e}");
            return page(
                502,
                "Planning Center refused the connection",
                "The approval could not be completed. Start again from lux.",
            );
        }
    };

    // Naming the church is a nicety, and a failure to do it must not cost a
    // church its connection — so this read is best-effort on purpose.
    let client = PcoClient::new(ctx.http.clone(), &tokens.access_token);
    let org = match client.organization().await {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("organization read failed, connecting anyway: {e}");
            lux_pco::Organization::default()
        }
    };

    let now_s = store::now_secs();
    let connection = Connection {
        org_id: org.id,
        org_name: org.name.clone(),
        access_token: tokens.access_token.clone(),
        refresh_token: tokens.refresh_token.clone(),
        access_expires_at_s: tokens.expires_at_s().unwrap_or(now_s),
        connected_at_ms: store::now_millis(),
        refresh_issued_at_s: if tokens.created_at > 0 {
            tokens.created_at
        } else {
            now_s
        },
    };
    if let Err(e) = store::put_connection(ctx, &sub, &connection).await {
        tracing::error!("connection write failed: {e}");
        return page(500, "Something went wrong", "Please try again from lux.");
    }

    tracing::info!("planning center connected");
    let church = org
        .name
        .map(|n| format!("{} is connected to lux.", escape(&n)))
        .unwrap_or_else(|| "Your Planning Center account is connected to lux.".to_owned());
    page(
        200,
        "Connected",
        &format!("{church} You can close this window and go back to lux."),
    )
}

/// `GET /pco/status` — what this account has connected, if anything.
pub async fn status(ctx: &Arc<Ctx>, event: &UrlEvent) -> Result<Value, Error> {
    let Some(sub) = caller(ctx, event) else {
        return reply(401, &error("unauthorized"));
    };
    match store::get_connection(ctx, &sub).await {
        Ok(None) => reply(200, &StatusResponse::default()),
        Ok(Some(conn)) => reply(
            200,
            &StatusResponse {
                connected: true,
                org_id: conn.org_id.clone(),
                org_name: conn.org_name.clone(),
                connected_at: (conn.connected_at_ms > 0).then_some(conn.connected_at_ms),
                needs_reconnect: conn.needs_reconnect(store::now_secs()),
            },
        ),
        Err(e) => {
            tracing::error!("status read failed: {e}");
            reply(500, &error("internal"))
        }
    }
}

/// `GET /pco/service-types` — the service types a setup could follow.
///
/// Retired ones are filtered out here rather than in the surface: an archived
/// service type is still returned by Planning Center, and binding a setup to
/// one produces a plan list that is permanently empty.
pub async fn service_types(ctx: &Arc<Ctx>, event: &UrlEvent) -> Result<Value, Error> {
    let client = match authed_client(ctx, event).await {
        Ok(c) => c,
        Err(response) => return response,
    };

    match client.service_types().await {
        Ok(types) => reply(
            200,
            &ServiceTypesResponse {
                service_types: types
                    .into_iter()
                    .filter(|t| !t.retired)
                    .map(|t| ServiceTypeSummary {
                        name: t.name.unwrap_or_else(|| format!("Service type {}", t.id)),
                        id: t.id,
                    })
                    .collect(),
            },
        ),
        Err(e) => pco_failure("service types", &e),
    }
}

/// `POST /pco/plan` — the next plan for a service type, resolved against the
/// cue map the caller sent.
pub async fn plan(ctx: &Arc<Ctx>, event: &UrlEvent) -> Result<Value, Error> {
    let req: PlanRequest = match body_bytes(event)
        .and_then(|b| serde_json::from_slice(&b).map_err(|e| format!("bad body: {e}")))
    {
        Ok(b) => b,
        Err(e) => return reply(400, &error(&e)),
    };
    if req.service_type_id.trim().is_empty() {
        return reply(400, &error("missing service type"));
    }
    // A map authored against another service type would resolve silently and
    // wrongly — the one mistake in this flow that produces confident nonsense.
    if let Some(map) = &req.cue_map {
        if map.service_type_id != req.service_type_id {
            return reply(400, &error("cue map is for a different service type"));
        }
    }

    let client = match authed_client(ctx, event).await {
        Ok(c) => c,
        Err(response) => return response,
    };

    let next = match client.next_plan(&req.service_type_id).await {
        Ok(p) => p,
        Err(e) => return pco_failure("next plan", &e),
    };
    // No future plan is Tuesday, not a failure.
    let Some(plan) = next else {
        return reply(200, &PlanResponse::default());
    };

    let items = match client.items(&req.service_type_id, &plan.id).await {
        Ok(i) => i,
        Err(e) => return pco_failure("plan items", &e),
    };

    let summaries = match &req.cue_map {
        Some(map) => CueSheet::resolve(map, &items)
            .cues()
            .iter()
            .map(|cue| PlanItemSummary {
                id: cue.item.id.clone(),
                sequence: cue.item.sequence,
                title: cue.item.title.clone(),
                item_type: cue.item.item_type.clone(),
                song_id: cue.item.song_id.clone(),
                length_s: cue.item.length_s,
                scene_id: cue.scene_id.clone(),
                source: cue.source.wire_name().to_owned(),
            })
            .collect(),
        // No map yet: the plan still reads, every row still shows, and manual
        // Go still works. That is the whole of the read-and-manual cut.
        None => {
            let mut items = items;
            items.sort_by(|a, b| a.sequence.cmp(&b.sequence).then_with(|| a.id.cmp(&b.id)));
            items
                .into_iter()
                .map(|item| PlanItemSummary {
                    id: item.id,
                    sequence: item.sequence,
                    title: item.title,
                    item_type: item.item_type,
                    song_id: item.song_id,
                    length_s: item.length_s,
                    scene_id: None,
                    source: lux_cue::CueSource::Unmapped.wire_name().to_owned(),
                })
                .collect()
        }
    };

    reply(
        200,
        &PlanResponse {
            plan: Some(PlanSummary {
                id: plan.id,
                dates: plan.dates.unwrap_or_default(),
                title: plan.title.or(plan.series_title),
                sort_date: plan.sort_date,
            }),
            items: summaries,
        },
    )
}

/// `POST /pco/disconnect` — forget the church's tokens.
///
/// Deletes rather than marks: the point of disconnecting is that lux stops
/// holding a credential for someone else's data, and a soft delete would not
/// be that.
pub async fn disconnect(ctx: &Arc<Ctx>, event: &UrlEvent) -> Result<Value, Error> {
    let Some(sub) = caller(ctx, event) else {
        return reply(401, &error("unauthorized"));
    };
    match store::delete_connection(ctx, &sub).await {
        Ok(()) => reply(200, &StatusResponse::default()),
        Err(e) => {
            tracing::error!("disconnect failed: {e}");
            reply(500, &error("internal"))
        }
    }
}

// --- shared plumbing ----------------------------------------------------------

type Error = lambda_runtime::Error;

/// Verify the caller, load their connection, refresh the access token if it is
/// close to expiring, and hand back a client pointed at Planning Center.
///
/// The `Err` arm is an already-formed HTTP response, so every caller's failure
/// wording stays in one place.
async fn authed_client(
    ctx: &Arc<Ctx>,
    event: &UrlEvent,
) -> Result<PcoClient<crate::SharedHttp>, Result<Value, Error>> {
    let Some(sub) = caller(ctx, event) else {
        return Err(reply(401, &error("unauthorized")));
    };
    let conn = match store::get_connection(ctx, &sub).await {
        Ok(Some(c)) => c,
        Ok(None) => return Err(reply(409, &error("planning center is not connected"))),
        Err(e) => {
            tracing::error!("connection read failed: {e}");
            return Err(reply(500, &error("internal")));
        }
    };

    let access_token = match tokens::fresh_access_token(ctx, &sub, &conn).await {
        Ok(t) => t,
        Err(tokens::Refused::Reconnect) => {
            return Err(reply(409, &error("planning center needs reconnecting")))
        }
        Err(tokens::Refused::Unavailable) => {
            return Err(reply(503, &error("planning center is unavailable")))
        }
    };
    Ok(PcoClient::new(ctx.http.clone(), access_token))
}

/// Translate a Planning Center failure into something a surface can act on.
///
/// Their response body never travels through: it is their wording about their
/// system, and a church reading it in a lux dialog would have no idea which of
/// two products was complaining.
fn pco_failure(what: &str, e: &PcoError) -> Result<Value, Error> {
    match e {
        PcoError::Unauthorized => {
            tracing::warn!("{what}: planning center rejected the token");
            reply(409, &error("planning center needs reconnecting"))
        }
        PcoError::RateLimited { retry_after_s } => {
            tracing::warn!("{what}: rate limited, retry after {retry_after_s:?}s");
            reply(
                429,
                &error("planning center is busy — try again in a moment"),
            )
        }
        other => {
            tracing::error!("{what} failed: {other}");
            reply(502, &error("could not reach planning center"))
        }
    }
}

/// A minimal, self-contained page. No external anything: this is served from a
/// Lambda into a browser that may be on a church's guest wifi.
fn page(status: u16, heading: &str, detail: &str) -> Result<Value, Error> {
    html(
        status,
        format!(
            "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<title>{heading} · lux</title>\
<style>:root{{color-scheme:light dark}}\
body{{font:16px/1.6 -apple-system,BlinkMacSystemFont,'Segoe UI',system-ui,sans-serif;\
margin:0;min-height:100vh;display:grid;place-items:center;padding:2rem}}\
main{{max-width:32rem;text-align:center}}\
h1{{font-size:1.5rem;margin:0 0 .5rem}}\
p{{margin:0;opacity:.8}}</style></head>\
<body><main><h1>{}</h1><p>{}</p></main></body></html>",
            escape(heading),
            escape(detail)
        ),
    )
}

/// Escape the five characters that matter in HTML text.
///
/// The church's own name is the only untrusted string that reaches the page,
/// and it comes from Planning Center rather than from us — so it gets escaped
/// like any other input, not trusted because of where it came from.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

/// 32 bytes of OS randomness, base64url. An error rather than a weak fallback:
/// a guessable `state` is a CSRF hole, so a randomness failure is a 500.
fn rand_token() -> Result<String, String> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use std::io::Read;
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| format!("no OS randomness: {e}"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_church_name_cannot_inject_markup_into_the_page() {
        // Planning Center is not the enemy, but the church's own display name
        // is user-entered text arriving over the network, and it lands in HTML.
        let injected = "<script>alert('x')</script>";
        let escaped = escape(injected);
        assert!(!escaped.contains('<'));
        assert_eq!(escaped, "&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;");

        let rendered =
            page(200, "Connected", &format!("{escaped} is connected.")).expect("page renders");
        let body = rendered["body"].as_str().expect("body is a string");
        assert!(!body.contains("<script>"));
    }

    #[test]
    fn ordinary_names_survive_escaping_readably() {
        assert_eq!(escape("Grace Chapel"), "Grace Chapel");
        assert_eq!(escape("Saints & Sinners"), "Saints &amp; Sinners");
    }

    #[test]
    fn a_state_token_is_urlsafe_and_never_repeats() {
        let a = rand_token().expect("randomness");
        let b = rand_token().expect("randomness");
        assert_ne!(a, b);
        assert!(a
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'));
    }

    #[test]
    fn planning_centers_failures_become_our_words_not_theirs() {
        let expired = pco_failure("test", &PcoError::Unauthorized).expect("reply");
        assert_eq!(expired["statusCode"], 409);
        let body = expired["body"].as_str().expect("body");
        assert!(body.contains("reconnect"));

        let busy = pco_failure(
            "test",
            &PcoError::RateLimited {
                retry_after_s: Some(20),
            },
        )
        .expect("reply");
        assert_eq!(busy["statusCode"], 429);

        // Their body must not travel through to a surface.
        let leaky = pco_failure(
            "test",
            &PcoError::Status {
                status: 500,
                detail: "planning center internal detail".into(),
            },
        )
        .expect("reply");
        assert_eq!(leaky["statusCode"], 502);
        assert!(!leaky["body"]
            .as_str()
            .expect("body")
            .contains("internal detail"));
    }
}
