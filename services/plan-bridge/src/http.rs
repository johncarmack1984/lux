//! The Function URL surface: parse the Lambda URL (payload v2) event, route,
//! and reply in `lux-wire` shapes.
//!
//! Identity on every route but the callback comes only from a verified Cognito
//! bearer token — a request body never names a user. The callback is the one
//! unauthenticated route, because the caller is Planning Center's redirect and
//! there is no bearer to carry; it authenticates instead on the single-use
//! `state` it hands back, which only this service could have minted.

use std::collections::HashMap;
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use lambda_runtime::Error;
use lux_wire::plan::{
    CALLBACK_SEGMENT, CONNECT_SEGMENT, DISCONNECT_SEGMENT, PCO_SEGMENT, PLAN_SEGMENT,
    SERVICE_TYPES_SEGMENT, STATUS_SEGMENT,
};
use lux_wire::ErrorResponse;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{routes, Ctx};

/// The slice of a Function URL (payload format 2.0) event we route on.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct UrlEvent {
    pub raw_path: String,
    pub headers: HashMap<String, String>,
    /// Already percent-decoded by the Function URL, and the only place the
    /// OAuth `code` and `state` arrive.
    pub query_string_parameters: HashMap<String, String>,
    pub body: Option<String>,
    pub is_base64_encoded: bool,
    request_context: RequestContext,
}

impl UrlEvent {
    pub fn query(&self, key: &str) -> Option<&str> {
        self.query_string_parameters
            .get(key)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RequestContext {
    http: HttpContext,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct HttpContext {
    method: String,
}

/// Which handler a request belongs to.
///
/// Separated from [`handle`] so the routing table is a pure function a test can
/// exercise directly. A test that re-implements the match instead can agree
/// with a bug in it — which is exactly how `/pco/plan` briefly answered `GET`
/// while the client sent `POST`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    Connect,
    Callback,
    Status,
    ServiceTypes,
    Plan,
    Disconnect,
}

pub(crate) fn route(method: &str, path: &str) -> Option<Route> {
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    let [prefix, tail] = segments.as_slice() else {
        return None;
    };
    if *prefix != PCO_SEGMENT {
        return None;
    }
    match (method, *tail) {
        ("POST", CONNECT_SEGMENT) => Some(Route::Connect),
        // Planning Center's redirect: a browser GET, and the only route here
        // that answers in HTML rather than JSON.
        ("GET", CALLBACK_SEGMENT) => Some(Route::Callback),
        ("GET", STATUS_SEGMENT) => Some(Route::Status),
        ("GET", SERVICE_TYPES_SEGMENT) => Some(Route::ServiceTypes),
        // A POST for a read, because the request carries the cue map — see
        // `lux_wire::plan::PlanRequest`.
        ("POST", PLAN_SEGMENT) => Some(Route::Plan),
        ("POST", DISCONNECT_SEGMENT) => Some(Route::Disconnect),
        _ => None,
    }
}

pub async fn handle(ctx: &Arc<Ctx>, payload: Value) -> Result<Value, Error> {
    let event: UrlEvent = match serde_json::from_value(payload) {
        Ok(e) => e,
        Err(e) => {
            tracing::error!("unroutable invoke payload: {e}");
            return reply(400, &error("unroutable request"));
        }
    };

    match route(&event.request_context.http.method, &event.raw_path) {
        Some(Route::Connect) => routes::connect(ctx, &event).await,
        Some(Route::Callback) => routes::callback(ctx, &event).await,
        Some(Route::Status) => routes::status(ctx, &event).await,
        Some(Route::ServiceTypes) => routes::service_types(ctx, &event).await,
        Some(Route::Plan) => routes::plan(ctx, &event).await,
        Some(Route::Disconnect) => routes::disconnect(ctx, &event).await,
        None => reply(404, &error("not found")),
    }
}

// --- request/response helpers -------------------------------------------------

/// The verified caller (`sub`) from the bearer token, if any.
pub(crate) fn caller(ctx: &Ctx, event: &UrlEvent) -> Option<String> {
    let token = event
        .headers
        .get("authorization")?
        .strip_prefix("Bearer ")?;
    ctx.verifier.verify(token).ok().map(|c| c.sub)
}

/// The request body, base64-decoded if the Function URL flagged it.
pub(crate) fn body_bytes(event: &UrlEvent) -> Result<Vec<u8>, String> {
    let raw = event.body.as_deref().unwrap_or_default();
    if event.is_base64_encoded {
        BASE64
            .decode(raw)
            .map_err(|e| format!("bad body encoding: {e}"))
    } else {
        Ok(raw.as_bytes().to_vec())
    }
}

pub(crate) fn error(message: &str) -> ErrorResponse {
    ErrorResponse {
        error: message.to_owned(),
    }
}

/// A Function URL (payload v2) JSON response.
pub(crate) fn reply<T: serde::Serialize>(status: u16, body: &T) -> Result<Value, Error> {
    Ok(json!({
        "statusCode": status,
        "headers": { "content-type": "application/json" },
        "body": serde_json::to_string(body)?,
    }))
}

/// An HTML page for the human standing in front of the browser at the end of
/// the OAuth dance. The one place this service renders anything.
pub(crate) fn html(status: u16, body: String) -> Result<Value, Error> {
    Ok(json!({
        "statusCode": status,
        "headers": {
            "content-type": "text/html; charset=utf-8",
            // The callback URL carries a single-use code in its query string.
            // Keep it out of caches and out of the next site's referer header.
            "cache-control": "no-store",
            "referrer-policy": "no-referrer",
        },
        "body": body,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(method: &str, path: &str) -> UrlEvent {
        UrlEvent {
            raw_path: path.into(),
            request_context: RequestContext {
                http: HttpContext {
                    method: method.into(),
                },
            },
            ..Default::default()
        }
    }

    #[test]
    fn the_callback_path_is_the_registered_one() {
        // Planning Center will only redirect to the URI registered on the
        // OAuth application, so the path this service routes on and the path
        // `lux-pco` registers have to be the same string.
        let registered = lux_pco::oauth::REDIRECT_URI_PROD;
        let path = registered
            .strip_prefix("https://auth.lux.johncarmack.com")
            .expect("prod callback is on the auth domain");
        assert_eq!(path, "/pco/callback");

        let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
        assert_eq!(segments.as_slice(), [PCO_SEGMENT, CALLBACK_SEGMENT]);
    }

    #[test]
    fn the_dev_callback_shares_the_path_so_one_router_serves_both() {
        let dev = lux_pco::oauth::REDIRECT_URI_DEV;
        assert!(dev.ends_with("/pco/callback"), "dev callback: {dev}");
    }

    #[test]
    fn every_route_the_client_calls_is_one_the_router_knows() {
        // The verbs here are the ones `apps/desktop/src-tauri/src/plan.rs`
        // actually sends. This is the pairing that matters: a table that
        // disagrees with the client 404s only in production.
        assert_eq!(route("POST", "/pco/connect"), Some(Route::Connect));
        assert_eq!(route("GET", "/pco/callback"), Some(Route::Callback));
        assert_eq!(route("GET", "/pco/status"), Some(Route::Status));
        assert_eq!(
            route("GET", "/pco/service-types"),
            Some(Route::ServiceTypes)
        );
        // A POST, because it carries the cue map.
        assert_eq!(route("POST", "/pco/plan"), Some(Route::Plan));
        assert_eq!(route("POST", "/pco/disconnect"), Some(Route::Disconnect));
    }

    #[test]
    fn an_unknown_route_is_a_404_not_a_panic() {
        assert_eq!(route("GET", "/nope"), None);
        assert_eq!(route("GET", "/"), None);
        assert_eq!(route("GET", ""), None);
        // Another service's path on the shared auth domain never lands here.
        assert_eq!(route("POST", "/auth/apple"), None);
        // Deeper and shallower paths are not near-misses.
        assert_eq!(route("GET", "/pco"), None);
        assert_eq!(route("GET", "/pco/plan/extra"), None);
    }

    #[test]
    fn the_wrong_verb_is_a_404_rather_than_the_wrong_handler() {
        // The callback is a browser GET; nothing else on it is.
        assert_eq!(route("POST", "/pco/callback"), None);
        // And the mutating routes are never reachable by GET, which is what
        // keeps a link or a prefetch from disconnecting a church.
        assert_eq!(route("GET", "/pco/connect"), None);
        assert_eq!(route("GET", "/pco/disconnect"), None);
        assert_eq!(route("GET", "/pco/plan"), None);
    }

    #[test]
    fn a_query_parameter_that_is_present_but_empty_counts_as_absent() {
        let mut e = event("GET", "/pco/callback");
        e.query_string_parameters
            .insert("code".into(), String::new());
        e.query_string_parameters
            .insert("state".into(), "st-1".into());
        assert_eq!(e.query("code"), None);
        assert_eq!(e.query("state"), Some("st-1"));
        assert_eq!(e.query("missing"), None);
    }

    #[test]
    fn the_callback_page_never_lands_in_a_cache_or_a_referer() {
        let page = html(200, "<p>ok</p>".into()).expect("html renders");
        let headers = &page["headers"];
        assert_eq!(headers["cache-control"], "no-store");
        assert_eq!(headers["referrer-policy"], "no-referrer");
    }
}
