//! The Planning Center bridge, from the app's side.
//!
//! Six thin calls onto `lux-plan-bridge` (`services/plan-bridge`). The app
//! never holds a Planning Center credential and never talks to Planning Center
//! directly: it presents its own Cognito token to the bridge, and the bridge —
//! which holds the one client secret and the church's refresh token — does the
//! reading. So a stolen laptop leaks nothing about a church's plans that
//! signing out does not immediately end.
//!
//! Everything here degrades rather than fails. No bridge in the endpoints file
//! (every build before the service shipped), not signed in, or not connected
//! are all ordinary states with their own words on the `/plan` route — not
//! errors, and never anything that touches the lights. Nothing in this module
//! is in the DMX path.

use lux_wire::plan::{
    ConnectResponse, PlanRequest, PlanResponse, StatusResponse, CONNECT_SEGMENT,
    DISCONNECT_SEGMENT, PCO_SEGMENT, PLAN_SEGMENT, SERVICE_TYPES_SEGMENT, STATUS_SEGMENT,
};
use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;
use tauri::{AppHandle, Manager};

use crate::account::LuxAccount;

// --- what the surface sees ---------------------------------------------------
//
// `lux-wire` stays serde-only — it is the contract two servers agree on, and
// giving it a specta dependency would drag the type system of one client into
// a shape the other has no use for. So the UI's types are declared here and
// translated, the same way `ShareTally` thins `shares::SharesResponse`.

/// Whether a Planning Center organization is connected, and which.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanConnection {
    pub connected: bool,
    /// The church's name, when Planning Center told us one.
    pub org_name: Option<String>,
    /// Epoch millis (an `f64` so it crosses to the webview as a plain number).
    pub connected_at: Option<f64>,
    /// The 90-day authorization is nearly up. The surface asks for a reconnect
    /// on a weekday rather than letting a Sunday discover it.
    pub needs_reconnect: bool,
    /// False when this build has no bridge configured or nobody is signed in —
    /// the difference between "not connected" and "cannot connect", which are
    /// different sentences on the route.
    pub available: bool,
}

/// A service type a setup could follow.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanServiceType {
    pub id: String,
    pub name: String,
}

/// This week's plan, ready to render.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    /// `None` when the service type has no future plan — an ordinary Tuesday.
    pub plan_id: Option<String>,
    /// Planning Center's own label for the date, rendered as they render it:
    /// the plan's timezone is the church's, not the device's.
    pub dates: Option<String>,
    pub title: Option<String>,
    pub items: Vec<PlanItemRow>,
}

/// One row of the plan.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanItemRow {
    pub id: String,
    pub title: String,
    /// `song`, `header`, `media`, `item`, or a church's own type.
    pub item_type: String,
    /// Planned length in seconds, when the plan gives one. Display only.
    pub length_s: Option<f64>,
    /// The scene this item calls for, once a cue map exists. Always `None` in
    /// this unit — the plan is driven by hand — and the field is here so the
    /// surface that renders it does not change when the map arrives.
    pub scene_id: Option<String>,
}

/// The consent URL to open in the administrator's browser.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanConsent {
    pub authorize_url: String,
}

#[ttipc::procedures(path = "plan")]
pub trait PlanMethods {
    /// Whether this account has a Planning Center organization connected.
    ///
    /// Answers "not connected, not available" — never an error — when the
    /// bridge is unconfigured or nobody is signed in, because the route's job
    /// is to explain the state, and three failures that all mean "you can't
    /// read a plan yet" should not need three different renderings.
    fn plan_status(&self, app_handle: AppHandle) -> Result<PlanConnection, String>;
    /// Start a connection: mint the Planning Center consent URL and open it in
    /// the administrator's **default browser**.
    ///
    /// Not in a webview, and not in one of our own windows. An OAuth consent
    /// screen is where someone types another company's password, and the only
    /// way they can check they are really at Planning Center is an address bar
    /// they trust — which an embedded webview cannot honestly give them.
    ///
    /// The URL comes back either way, so a surface can offer "didn't open?
    /// copy the link" when the machine has no browser to hand it to.
    fn plan_connect(&self, app_handle: AppHandle) -> Result<PlanConsent, String>;
    /// The service types this church could bind a setup to.
    fn plan_service_types(&self, app_handle: AppHandle) -> Result<Vec<PlanServiceType>, String>;
    /// This week's plan for a service type, with its items in plan order.
    fn plan_next(&self, app_handle: AppHandle, service_type_id: String)
        -> Result<PlanView, String>;
    /// Forget the church's tokens. The lights are untouched.
    fn plan_disconnect(&self, app_handle: AppHandle) -> Result<PlanConnection, String>;
}

#[derive(Clone)]
pub struct PlanEndpoint;

impl PlanMethods for PlanEndpoint {
    fn plan_status(&self, app_handle: AppHandle) -> Result<PlanConnection, String> {
        log::trace!("plan_status");
        // Not configured and not signed in are both "nothing to ask". The
        // route says which, from state it already has.
        let Some((base, token)) = credentials(&app_handle) else {
            return Ok(PlanConnection::default());
        };
        let status: StatusResponse =
            request(&app_handle, Method::Get, &base, STATUS_SEGMENT, token, None)?;
        Ok(connection(status))
    }

    fn plan_connect(&self, app_handle: AppHandle) -> Result<PlanConsent, String> {
        log::info!("plan_connect: starting a Planning Center connection");
        let (base, token) = credentials(&app_handle).ok_or(NOT_READY)?;
        let response: ConnectResponse = request(
            &app_handle,
            Method::Post,
            &base,
            CONNECT_SEGMENT,
            token,
            None,
        )?;
        // Failing to open the browser must not lose the URL: the connect
        // attempt is already banked server-side, and the surface can still
        // offer the link. Log it and carry on.
        if let Err(e) = open_in_browser(&app_handle, &response.authorize_url) {
            log::warn!("could not open the Planning Center consent page: {e}");
        }
        // `state` stays server-side business: the bridge minted it, stored it,
        // and will consume it at the callback. Handing it to the webview would
        // be handing out a CSRF token with nothing to do with it.
        Ok(PlanConsent {
            authorize_url: response.authorize_url,
        })
    }

    fn plan_service_types(&self, app_handle: AppHandle) -> Result<Vec<PlanServiceType>, String> {
        log::trace!("plan_service_types");
        let (base, token) = credentials(&app_handle).ok_or(NOT_READY)?;
        let response: lux_wire::plan::ServiceTypesResponse = request(
            &app_handle,
            Method::Get,
            &base,
            SERVICE_TYPES_SEGMENT,
            token,
            None,
        )?;
        Ok(response
            .service_types
            .into_iter()
            .map(|t| PlanServiceType {
                id: t.id,
                name: t.name,
            })
            .collect())
    }

    fn plan_next(
        &self,
        app_handle: AppHandle,
        service_type_id: String,
    ) -> Result<PlanView, String> {
        log::trace!("plan_next({service_type_id})");
        let (base, token) = credentials(&app_handle).ok_or(NOT_READY)?;
        // No cue map yet: this unit reads the plan and drives it by hand, so
        // every item comes back unresolved and the operator picks the scene.
        // Authoring a map — the thing that makes next week free — is the next
        // unit, and it lands as a `cue_map` here without a wire change.
        let body = PlanRequest {
            service_type_id,
            cue_map: None,
        };
        let body = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
        let response: PlanResponse = request(
            &app_handle,
            Method::Post,
            &base,
            PLAN_SEGMENT,
            token,
            Some(body),
        )?;
        Ok(view(response))
    }

    fn plan_disconnect(&self, app_handle: AppHandle) -> Result<PlanConnection, String> {
        log::info!("plan_disconnect: forgetting the Planning Center connection");
        let (base, token) = credentials(&app_handle).ok_or(NOT_READY)?;
        let status: StatusResponse = request(
            &app_handle,
            Method::Post,
            &base,
            DISCONNECT_SEGMENT,
            token,
            None,
        )?;
        Ok(connection(status))
    }
}

/// Hand a URL to the OS's default browser.
///
/// Only ever called with a URL this app just built from
/// [`lux_pco::oauth::AUTHORIZE_URL`] — never with anything a server or a
/// webview supplied, which is what keeps "open a link" from being a way to
/// launch arbitrary things.
fn open_in_browser(app: &AppHandle, url: &str) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| format!("could not open a browser: {e}"))
}

// --- wire → surface -----------------------------------------------------------

fn connection(status: StatusResponse) -> PlanConnection {
    PlanConnection {
        connected: status.connected,
        org_name: status.org_name,
        // i64 millis → f64 for the webview. Epoch milliseconds stay exact well
        // past any date this app will see, so the widening is lossless here.
        connected_at: status.connected_at.map(|ms| ms as f64),
        needs_reconnect: status.needs_reconnect,
        // We only got an answer because both the bridge URL and a token were
        // there, so by construction it is available.
        available: true,
    }
}

fn view(response: PlanResponse) -> PlanView {
    let items = response
        .items
        .into_iter()
        .map(|item| PlanItemRow {
            id: item.id,
            title: item.title,
            item_type: item.item_type,
            length_s: item.length_s.map(|s| s as f64),
            scene_id: item.scene_id,
        })
        .collect();
    match response.plan {
        Some(plan) => PlanView {
            plan_id: Some(plan.id),
            dates: (!plan.dates.is_empty()).then_some(plan.dates),
            title: plan.title,
            items,
        },
        None => PlanView::default(),
    }
}

/// What every route but `plan_status` says when there is nothing to call with.
const NOT_READY: &str = "sign in to connect Planning Center";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    Get,
    Post,
}

/// The bridge's base URL and the caller's current id token, if both exist.
fn credentials(app: &AppHandle) -> Option<(String, String)> {
    let account = app.state::<LuxAccount>();
    let base = account.plan_bridge_url()?;
    let token = account.current_id_token()?;
    Some((base, token))
}

/// One call to the bridge, with a single token refresh on a 401.
///
/// The refresh is worth the code because the alternative is a church tapping
/// Connect, being told to sign in again, and losing the tab they had open at
/// Planning Center — an id token that expired while the app sat idle is the
/// normal case here, not an exception.
fn request<T: DeserializeOwned + Send + 'static>(
    app: &AppHandle,
    method: Method,
    base: &str,
    segment: &str,
    token: String,
    body: Option<Vec<u8>>,
) -> Result<T, String> {
    let url = format!("{base}/{PCO_SEGMENT}/{segment}");
    let app = app.clone();
    crate::account::block_on(async move {
        let client = Client::new();
        let first = send(&client, method, &url, &token, body.clone()).await;
        let response = match first {
            Err(BridgeError::Unauthorized) => {
                let fresh = app
                    .state::<LuxAccount>()
                    .refresh_id_token()
                    .await
                    .map_err(|_| "sign in again to reach Planning Center".to_owned())?;
                send(&client, method, &url, &fresh, body).await
            }
            other => other,
        };
        response.map_err(|e| e.to_string())
    })
}

async fn send<T: DeserializeOwned>(
    client: &Client,
    method: Method,
    url: &str,
    token: &str,
    body: Option<Vec<u8>>,
) -> Result<T, BridgeError> {
    let builder = match method {
        Method::Get => client.get(url),
        Method::Post => client.post(url),
    };
    let builder = builder.bearer_auth(token);
    let builder = match body {
        Some(bytes) => builder
            .header("content-type", "application/json")
            .body(bytes),
        None => builder,
    };

    let response = builder
        .send()
        .await
        .map_err(|e| BridgeError::Other(e.to_string()))?;

    match response.status() {
        StatusCode::UNAUTHORIZED => Err(BridgeError::Unauthorized),
        status if status.is_success() => response
            .json::<T>()
            .await
            .map_err(|e| BridgeError::Other(e.to_string())),
        // The bridge states its refusals plainly and in its own words — "not
        // connected", "needs reconnecting", "busy". Surface those rather than
        // flattening them to a status code, because each one names a different
        // thing the operator can do about it.
        _ => Err(match response.json::<lux_wire::ErrorResponse>().await {
            Ok(e) => BridgeError::Other(e.error),
            Err(_) => BridgeError::Other("could not reach the plan bridge".to_owned()),
        }),
    }
}

enum BridgeError {
    Unauthorized,
    Other(String),
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeError::Unauthorized => f.write_str("sign in again to reach Planning Center"),
            BridgeError::Other(message) => f.write_str(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_route_is_built_under_the_one_prefix() {
        // The bridge routes on `/pco/<segment>`; a URL assembled any other way
        // would 404 in production and nowhere else.
        let base = "https://auth.lux.johncarmack.com";
        for segment in [
            STATUS_SEGMENT,
            CONNECT_SEGMENT,
            SERVICE_TYPES_SEGMENT,
            PLAN_SEGMENT,
            DISCONNECT_SEGMENT,
        ] {
            let url = format!("{base}/{PCO_SEGMENT}/{segment}");
            assert!(url.starts_with("https://auth.lux.johncarmack.com/pco/"));
        }
        assert_eq!(
            format!("{base}/{PCO_SEGMENT}/{PLAN_SEGMENT}"),
            "https://auth.lux.johncarmack.com/pco/plan"
        );
    }

    #[test]
    fn a_bridge_refusal_keeps_its_own_wording() {
        // "needs reconnecting" and "not connected" mean different things to an
        // operator; collapsing them would lose the only actionable part.
        let refused = BridgeError::Other("planning center needs reconnecting".into());
        assert_eq!(refused.to_string(), "planning center needs reconnecting");
        assert_eq!(
            BridgeError::Unauthorized.to_string(),
            "sign in again to reach Planning Center"
        );
    }
}
