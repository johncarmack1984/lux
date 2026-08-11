//! The bridge, end to end, against recorded Planning Center responses.
//!
//! Plan in → cue map resolves → simulated live positions → the scenes that
//! come out. Every JSON body in `fixtures/` is the shape the published
//! Services API documents; the transport is a recording, so the whole path —
//! paging, the double hop from Live to a plan item, a 429, an expired token,
//! a service that ends — runs in `cargo test` and not for the first time on a
//! Sunday.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use lux_cue::{CueSheet, Follower, Observation, Outcome, Status};
use lux_pco::http::{BoxFuture, Http, HttpRequest, HttpResponse, Method};
use lux_pco::{Error, LiveSlot, OAuthApp, PcoClient, PlanFilter};
use lux_wire::plan::{CueMap, CueRule, TitleMode};

const BASE: &str = "https://pco.test";
const SERVICE_TYPE: &str = "1109432";
const PLAN: &str = "77123";
const LIVE: &str = "77123";

const SERVICE_TYPES: &str = include_str!("fixtures/service_types.json");
const PLANS_FUTURE: &str = include_str!("fixtures/plans_future.json");
const ITEMS_PAGE_1: &str = include_str!("fixtures/items_page_1.json");
const ITEMS_PAGE_2: &str = include_str!("fixtures/items_page_2.json");
const LIVE_PRE_SERVICE: &str = include_str!("fixtures/live_pre_service.json");
const LIVE_SONG: &str = include_str!("fixtures/live_song.json");
const LIVE_SERMON: &str = include_str!("fixtures/live_sermon.json");
const LIVE_DOXOLOGY: &str = include_str!("fixtures/live_doxology.json");
const LIVE_ENDED: &str = include_str!("fixtures/live_ended.json");
const LIVE_INCLUDE_IGNORED: &str = include_str!("fixtures/live_include_ignored.json");
const LIVE_CURRENT_ITEM_TIME: &str = include_str!("fixtures/live_current_item_time.json");
const LIVE_CURRENT_ITEM_TIME_NULL: &str = include_str!("fixtures/live_current_item_time_null.json");
const TOKEN: &str = include_str!("fixtures/token.json");
const ERROR_429: &str = include_str!("fixtures/error_429.json");
const ERROR_401: &str = include_str!("fixtures/error_401.json");

// --- the recording ----------------------------------------------------------

/// A transport that answers from recorded responses and remembers what it was
/// asked for. A URL with several responses queued serves them in order and
/// then repeats the last one, which is what makes a two-second poll loop
/// expressible as a list of fixtures.
#[derive(Default)]
struct Recorded {
    routes: Mutex<BTreeMap<String, VecDeque<HttpResponse>>>,
    calls: Mutex<Vec<(Method, String)>>,
    /// The form bodies the OAuth hops sent, in order. Kept apart from `calls`
    /// because only those hops have one, and the read path's assertion is
    /// precisely that it never does.
    bodies: Mutex<Vec<String>>,
}

impl Recorded {
    fn new() -> Self {
        Self::default()
    }

    fn on(self, url: impl Into<String>, response: HttpResponse) -> Self {
        if let Ok(mut routes) = self.routes.lock() {
            routes.entry(url.into()).or_default().push_back(response);
        }
        self
    }

    fn ok(self, url: impl Into<String>, body: &str) -> Self {
        self.on(url, HttpResponse::new(200, body))
    }

    fn calls(&self) -> Vec<(Method, String)> {
        self.calls.lock().map(|c| c.clone()).unwrap_or_default()
    }

    fn bodies(&self) -> Vec<String> {
        self.bodies.lock().map(|b| b.clone()).unwrap_or_default()
    }
}

impl Http for Recorded {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, Error>> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push((request.method, request.url.clone()));
        }
        if let (Ok(mut bodies), Some(body)) = (self.bodies.lock(), request.body.as_ref()) {
            bodies.push(body.clone());
        }
        let answer = match self.routes.lock() {
            Ok(mut routes) => match routes.get_mut(&request.url) {
                Some(queued) if queued.len() > 1 => queued.pop_front(),
                Some(queued) => queued.front().cloned(),
                None => None,
            },
            Err(_) => None,
        };
        Box::pin(async move {
            answer.ok_or_else(|| Error::Transport(format!("no fixture for {}", request.url)))
        })
    }
}

fn service_types_url() -> String {
    format!("{BASE}/services/v2/service_types?per_page=100")
}

fn next_plan_url() -> String {
    format!("{BASE}/services/v2/service_types/{SERVICE_TYPE}/plans?filter=future&order=sort_date&per_page=1")
}

fn items_url() -> String {
    format!("{BASE}/services/v2/service_types/{SERVICE_TYPE}/plans/{PLAN}/items?per_page=100")
}

/// The `links.next` the recorded first page carries. The two pages were
/// captured at `per_page=5` so a paged read fits in a fixture; the client
/// follows the link it is given, whatever page size it asked for.
fn items_page_2_url() -> String {
    format!(
        "{BASE}/services/v2/service_types/{SERVICE_TYPE}/plans/{PLAN}/items?offset=5&per_page=5"
    )
}

fn live_url() -> String {
    format!("{BASE}/services/v2/service_types/{SERVICE_TYPE}/plans/{PLAN}/live?include=current_item_time,next_item_time")
}

fn association_url(slot: LiveSlot) -> String {
    format!(
        "{BASE}/services/v2/service_types/{SERVICE_TYPE}/plans/{PLAN}/live/{LIVE}/{}?include=item",
        slot.as_str()
    )
}

/// The map a worship pastor would author once, against the service type.
fn cue_map() -> CueMap {
    CueMap::new(
        SERVICE_TYPE.into(),
        vec![
            CueRule::ItemType {
                item_type: "song".into(),
                scene_id: "worship".into(),
            },
            CueRule::ItemType {
                item_type: "media".into(),
                scene_id: "video".into(),
            },
            CueRule::Title {
                pattern: "announcements".into(),
                mode: TitleMode::Contains,
                scene_id: "announce".into(),
            },
            CueRule::Title {
                pattern: "sermon".into(),
                mode: TitleMode::Contains,
                scene_id: "sermon".into(),
            },
            CueRule::Pin {
                song_id: "1003".into(),
                scene_id: "doxology".into(),
            },
        ],
    )
    .with_fallback("house".into())
}

// --- the acceptance run -----------------------------------------------------

#[tokio::test]
async fn a_service_runs_from_the_plan_to_the_scenes() {
    let http = Recorded::new()
        .ok(service_types_url(), SERVICE_TYPES)
        .ok(next_plan_url(), PLANS_FUTURE)
        .ok(items_url(), ITEMS_PAGE_1)
        .ok(items_page_2_url(), ITEMS_PAGE_2)
        // One entry per poll: pre-service twice (nothing must fire twice),
        // then the room skips announcements and the video and lands on the
        // sermon, then the doxology, then the service ends.
        .ok(live_url(), LIVE_PRE_SERVICE)
        .ok(live_url(), LIVE_PRE_SERVICE)
        .ok(live_url(), LIVE_SONG)
        .ok(live_url(), LIVE_SONG)
        .ok(live_url(), LIVE_SERMON)
        .ok(live_url(), LIVE_DOXOLOGY)
        .ok(live_url(), LIVE_ENDED);

    let client = PcoClient::new(http, "access-token").with_base(BASE);

    // 1. The church picks a service type. The archived one is flagged, not
    //    hidden — the surface decides, but it must be able to.
    let service_types = client.service_types().await.unwrap();
    assert_eq!(service_types.len(), 2);
    assert_eq!(service_types[0].name.as_deref(), Some("Sunday 9:00"));
    assert!(!service_types[0].retired);
    assert!(service_types[1].retired);

    // 2. This Sunday's plan.
    let plan = client.next_plan(SERVICE_TYPE).await.unwrap().unwrap();
    assert_eq!(plan.id, PLAN);
    assert_eq!(plan.dates.as_deref(), Some("August 9, 2026"));
    assert_eq!(plan.title, None); // untitled plans are the norm

    // 3. Its items — both pages, in plan order, songs carrying their library id.
    let items = client.items(SERVICE_TYPE, PLAN).await.unwrap();
    assert_eq!(items.len(), 8);
    assert_eq!(items[1].title, "Great Are You Lord");
    assert_eq!(items[1].song_id.as_deref(), Some("1001"));
    assert_eq!(items[1].length_s, Some(280));
    assert_eq!(items[7].title, "Dismissal");

    // 4. The map resolves the whole plan before anything is live.
    let sheet = CueSheet::resolve(&cue_map(), &items);
    assert_eq!(
        sheet
            .cues()
            .iter()
            .map(|c| c.scene_id.as_deref())
            .collect::<Vec<_>>(),
        vec![
            Some("house"),    // Pre-Service
            Some("worship"),  // Great Are You Lord
            Some("worship"),  // King of Kings
            Some("announce"), // Welcome & Announcements
            Some("video"),    // Bumper Video
            Some("sermon"),   // Sermon — Part 3
            Some("doxology"), // Doxology, pinned by song
            Some("house"),    // Dismissal
        ]
    );

    // 5. Follow live: seven polls two seconds apart.
    let mut follower = Follower::new(sheet);
    let mut fired = Vec::new();
    let mut statuses = Vec::new();
    for tick in 0..7 {
        let observation = match client.live(SERVICE_TYPE, PLAN).await {
            Ok(snapshot) => Observation::Live(snapshot.position()),
            Err(_) => Observation::PollFailed,
        };
        let decision = follower.observe(observation, tick * 2_000);
        if let Outcome::Fire { scene_id } = decision.outcome {
            fired.push(scene_id);
        }
        statuses.push(decision.status);
    }

    // The announcements and the video were in the plan and never happened in
    // the room; nothing replayed them on the way past.
    assert_eq!(fired, vec!["house", "worship", "sermon", "doxology"]);
    // The service ended: the doxology look is still up.
    assert_eq!(statuses.last(), Some(&Status::Idle));
    assert_eq!(follower.scene_on_rig(), Some("doxology"));
}

// --- the client's own promises ---------------------------------------------

#[tokio::test]
async fn a_poll_is_exactly_one_request_and_never_a_write() {
    let http = Recorded::new().ok(live_url(), LIVE_SONG);
    let client = PcoClient::new(http, "access-token").with_base(BASE);

    let snapshot = client.live(SERVICE_TYPE, PLAN).await.unwrap();
    assert_eq!(snapshot.current_item_id(), Some("9002"));
    assert_eq!(snapshot.next_item_id(), Some("9003"));
    assert!(snapshot.can_control); // read, shown, never acted on
    assert!(!snapshot.has_unresolved_pointer());

    let calls = client.http().calls();
    assert_eq!(calls.len(), 1, "a poll of the live vertex is one request");
    assert!(
        calls.iter().all(|(method, _)| *method == Method::Get),
        "the read client issues nothing but GET: {calls:?}"
    );
}

#[tokio::test]
async fn every_request_the_client_makes_is_a_get() {
    // The whole surface of the client, exercised: if a write ever appears on
    // this type, this test is what fails.
    let http = Recorded::new()
        .ok(service_types_url(), SERVICE_TYPES)
        .ok(next_plan_url(), PLANS_FUTURE)
        .ok(
            format!("{BASE}/services/v2/service_types/{SERVICE_TYPE}/plans?per_page=100&order=sort_date&filter=future"),
            PLANS_FUTURE,
        )
        .ok(items_url(), ITEMS_PAGE_1)
        .ok(items_page_2_url(), ITEMS_PAGE_2)
        .ok(live_url(), LIVE_SONG)
        .ok(association_url(LiveSlot::Current), LIVE_CURRENT_ITEM_TIME);

    let client = PcoClient::new(http, "access-token").with_base(BASE);
    client.service_types().await.unwrap();
    client.next_plan(SERVICE_TYPE).await.unwrap();
    client
        .plans(SERVICE_TYPE, PlanFilter::Future)
        .await
        .unwrap();
    client.items(SERVICE_TYPE, PLAN).await.unwrap();
    client.live(SERVICE_TYPE, PLAN).await.unwrap();
    client
        .live_item_time(SERVICE_TYPE, PLAN, LIVE, LiveSlot::Current)
        .await
        .unwrap();

    let calls = client.http().calls();
    assert!(calls.len() >= 7);
    assert!(calls.iter().all(|(method, _)| *method == Method::Get));
}

#[tokio::test]
async fn a_live_vertex_that_ignores_the_include_falls_back_to_the_association() {
    // The single-request poll depends on Planning Center sideloading the two
    // ItemTimes. If a deployment ever doesn't, the pointers still come back —
    // and the association endpoint answers the same question in one more hop.
    let http = Recorded::new()
        .ok(live_url(), LIVE_INCLUDE_IGNORED)
        .ok(association_url(LiveSlot::Current), LIVE_CURRENT_ITEM_TIME);
    let client = PcoClient::new(http, "access-token").with_base(BASE);

    let snapshot = client.live(SERVICE_TYPE, PLAN).await.unwrap();
    assert!(snapshot.has_unresolved_pointer());
    assert_eq!(snapshot.current_item_id(), None);
    assert_eq!(snapshot.current_item_time_id.as_deref(), Some("88002"));

    let item_time = client
        .live_item_time(SERVICE_TYPE, PLAN, &snapshot.id, LiveSlot::Current)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(item_time.item_id.as_deref(), Some("9002"));
}

#[tokio::test]
async fn an_association_pointing_at_nothing_is_not_an_error() {
    let http = Recorded::new().ok(association_url(LiveSlot::Next), LIVE_CURRENT_ITEM_TIME_NULL);
    let client = PcoClient::new(http, "access-token").with_base(BASE);
    let item_time = client
        .live_item_time(SERVICE_TYPE, PLAN, LIVE, LiveSlot::Next)
        .await
        .unwrap();
    assert_eq!(item_time, None);
}

#[tokio::test]
async fn a_service_that_has_not_started_holds_rather_than_lighting_anything() {
    let http = Recorded::new().ok(live_url(), LIVE_ENDED);
    let client = PcoClient::new(http, "access-token").with_base(BASE);
    let snapshot = client.live(SERVICE_TYPE, PLAN).await.unwrap();
    assert_eq!(snapshot.position().current_item_id, None);
    assert!(!snapshot.can_control);

    let mut follower = Follower::new(CueSheet::resolve(&cue_map(), &[]));
    let decision = follower.observe(Observation::Live(snapshot.position()), 0);
    assert_eq!(decision.outcome, Outcome::Hold);
    assert_eq!(decision.status, Status::Idle);
}

#[tokio::test]
async fn the_rate_limit_headers_are_read_from_every_response() {
    let http = Recorded::new().on(
        live_url(),
        HttpResponse::new(200, LIVE_SONG)
            .with_header("X-PCO-API-Request-Rate-Limit", "100")
            .with_header("X-PCO-API-Request-Rate-Period", "20")
            .with_header("X-PCO-API-Request-Rate-Count", "11"),
    );
    let client = PcoClient::new(http, "access-token").with_base(BASE);
    assert_eq!(client.rate_limit(), None);

    client.live(SERVICE_TYPE, PLAN).await.unwrap();
    let seen = client.rate_limit().unwrap();
    assert_eq!(seen.limit, 100);
    assert_eq!(seen.count, 11);
    // A two-second poll of one plan spends ten of a hundred.
    assert_eq!(seen.remaining(), 89);
}

#[tokio::test]
async fn a_429_carries_the_servers_own_retry_after_and_the_lights_hold() {
    let http = Recorded::new().on(
        live_url(),
        HttpResponse::new(429, ERROR_429).with_header("Retry-After", "20"),
    );
    let client = PcoClient::new(http, "access-token").with_base(BASE);

    let error = client.live(SERVICE_TYPE, PLAN).await.unwrap_err();
    assert!(
        matches!(
            error,
            Error::RateLimited {
                retry_after_s: Some(20)
            }
        ),
        "expected the server's own Retry-After, got {error:?}"
    );

    // Whatever the failure was, the follower's answer is the same one.
    let mut follower = Follower::new(CueSheet::resolve(&cue_map(), &[]));
    assert_eq!(
        follower.observe(Observation::PollFailed, 0).outcome,
        Outcome::Hold
    );
}

#[tokio::test]
async fn an_expired_token_is_told_apart_from_every_other_failure() {
    // The poller has to know this one: it means refresh and retry, not back
    // off. Everything else is "try again on the next tick".
    let http = Recorded::new()
        .on(live_url(), HttpResponse::new(401, ERROR_401))
        .on(
            items_url(),
            HttpResponse::new(500, "upstream is having a day"),
        );
    let client = PcoClient::new(http, "access-token").with_base(BASE);

    assert!(matches!(
        client.live(SERVICE_TYPE, PLAN).await.unwrap_err(),
        Error::Unauthorized
    ));
    let error = client.items(SERVICE_TYPE, PLAN).await.unwrap_err();
    assert!(
        matches!(&error, Error::Status { status: 500, detail } if detail.contains("having a day")),
        "expected the upstream status to survive, got {error:?}"
    );
}

#[tokio::test]
async fn a_body_that_is_not_json_is_a_decode_error_not_a_panic() {
    let http = Recorded::new().on(
        live_url(),
        HttpResponse::new(200, "<html>maintenance</html>"),
    );
    let client = PcoClient::new(http, "access-token").with_base(BASE);
    assert!(matches!(
        client.live(SERVICE_TYPE, PLAN).await.unwrap_err(),
        Error::Decode(_)
    ));
}

// --- the connect dance ------------------------------------------------------

#[tokio::test]
async fn the_church_connects_once_and_the_token_refreshes_itself() {
    let http = Recorded::new()
        .ok(lux_pco::oauth::TOKEN_URL, TOKEN)
        .ok(lux_pco::oauth::TOKEN_URL, TOKEN);
    let app = OAuthApp::new(
        "client-id",
        "client-secret",
        lux_pco::oauth::REDIRECT_URI_PROD,
    );

    let tokens = app.exchange_code(&http, "the-code").await.unwrap();
    assert_eq!(tokens.scope, "services");
    assert_eq!(tokens.expires_at_s(), Some(1_786_291_200 + 7_200));
    assert!(!tokens.needs_refresh(1_786_291_300));
    assert!(tokens.needs_refresh(1_786_298_200));

    let renewed = app.refresh(&http, &tokens.refresh_token).await.unwrap();
    assert_eq!(renewed.access_token, tokens.access_token);

    // Both hops are form-encoded POSTs to the token endpoint, and nothing
    // else in this crate posts anywhere.
    let calls = http.calls();
    assert_eq!(calls.len(), 2);
    assert!(calls
        .iter()
        .all(|(method, url)| *method == Method::Post && url == lux_pco::oauth::TOKEN_URL));
}

#[tokio::test]
async fn a_rejected_code_is_reported_not_swallowed() {
    let http = Recorded::new().on(
        lux_pco::oauth::TOKEN_URL,
        HttpResponse::new(400, r#"{"error":"invalid_grant"}"#),
    );
    let app = OAuthApp::new(
        "client-id",
        "client-secret",
        lux_pco::oauth::REDIRECT_URI_DEV,
    );
    let error = app.exchange_code(&http, "stale-code").await.unwrap_err();
    assert!(
        matches!(&error, Error::Status { status: 400, detail } if detail.contains("invalid_grant")),
        "expected the OAuth error to survive, got {error:?}"
    );
}

// --- handing the grant back -------------------------------------------------

fn revoking_app() -> OAuthApp {
    OAuthApp::new(
        "client-id",
        "client-secret",
        lux_pco::oauth::REDIRECT_URI_PROD,
    )
}

#[tokio::test]
async fn a_disconnect_hands_the_refresh_token_back_to_planning_center() {
    // Planning Center answers a successful revocation with 200 and an empty
    // JSON body.
    let http = Recorded::new().ok(lux_pco::oauth::REVOKE_URL, "{}");

    revoking_app()
        .revoke(&http, "the-churchs-refresh-token")
        .await
        .expect("a 200 is a revocation");

    // One POST, to the revocation endpoint, naming the refresh token as a
    // refresh token — the hint is what makes this end the whole grant rather
    // than one two-hour access token, which would leave the 90-day credential
    // alive and the defect unfixed.
    let calls = http.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, Method::Post);
    assert_eq!(calls[0].1, lux_pco::oauth::REVOKE_URL);

    let body = http.bodies().pop().expect("the revocation carried a body");
    assert!(body.contains("token=the-churchs-refresh-token"), "{body}");
    assert!(body.contains("token_type_hint=refresh_token"), "{body}");
    assert!(body.contains("client_id=client-id"), "{body}");
    assert!(body.contains("client_secret=client-secret"), "{body}");
}

#[tokio::test]
async fn revoking_a_token_that_is_already_gone_is_a_success() {
    // Their endpoint answers 200 for a token that was already revoked or never
    // existed. That is the property that makes an account deletion safe to
    // retry: "revoked" here means "cannot be spent", not "was live a moment
    // ago".
    let http = Recorded::new().ok(lux_pco::oauth::REVOKE_URL, "{}");
    assert!(revoking_app().revoke(&http, "spent-token").await.is_ok());
}

#[tokio::test]
async fn a_refused_revocation_is_reported_rather_than_swallowed() {
    // The caller deleting an account carries on regardless — but it can only
    // log what it is told, so a failure has to arrive as one.
    let http = Recorded::new().on(
        lux_pco::oauth::REVOKE_URL,
        HttpResponse::new(500, r#"{"error":"server_error"}"#),
    );
    let error = revoking_app()
        .revoke(&http, "the-churchs-refresh-token")
        .await
        .unwrap_err();
    assert!(
        matches!(&error, Error::Status { status: 500, .. }),
        "expected the refusal to survive, got {error:?}"
    );

    // And an unreachable Planning Center is a transport error, not a silent ok.
    let offline = Recorded::new();
    assert!(revoking_app().revoke(&offline, "rt").await.is_err());
}
