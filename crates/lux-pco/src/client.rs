//! The read client.
//!
//! Every method here builds a `GET`. There is no `post`, `put` or `delete` on
//! this type, and no method named for one of the Live vertex's write actions —
//! which is the whole of lux's "never advance someone else's service"
//! guarantee, since the OAuth scope cannot express it (see the crate docs).
//!
//! Two other things this type owns, because every caller would otherwise get
//! them subtly wrong:
//!
//! - **Paging.** Planning Center answers a list with `links.next` when there
//!   is more. A plan's items are read to exhaustion (bounded by
//!   [`MAX_PAGES`]), so a long service isn't silently truncated at page one.
//! - **Rate limits.** Every response's `X-PCO-API-Request-Rate-*` headers are
//!   recorded on the client ([`PcoClient::rate_limit`]) and a 429 carries the
//!   server's own `Retry-After` rather than a guess. The documented ceiling
//!   may be adjusted without notice, so the headers are the truth and the
//!   number in the docs is a rule of thumb.

use std::sync::Mutex;

use serde::de::DeserializeOwned;

use crate::error::Error;
use crate::http::{Http, HttpRequest};
use crate::jsonapi::{Collection, MaybeSingle, Resource, Single};
use crate::services::{
    plan_item, ItemAttrs, ItemTime, ItemTimeAttrs, LiveAttrs, LiveSlot, LiveSnapshot, Plan,
    PlanAttrs, ServiceType, ServiceTypeAttrs,
};
use lux_cue::PlanItem;

pub const API_BASE: &str = "https://api.planningcenteronline.com";

/// How many pages one list read will follow. A service plan of 2,000 rows is
/// not a service plan, and an unbounded loop against a paginated API is how a
/// poller becomes an outage.
pub const MAX_PAGES: usize = 20;

/// The largest page Planning Center serves.
const PER_PAGE: usize = 100;

/// What the rate-limit headers said on the last response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimit {
    /// Requests allowed in the period.
    pub limit: u32,
    /// Requests spent so far in it.
    pub count: u32,
}

impl RateLimit {
    pub fn remaining(&self) -> u32 {
        self.limit.saturating_sub(self.count)
    }
}

/// Which plans to list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanFilter {
    /// Today's and later — what a bridge almost always wants.
    Future,
    Past,
    All,
}

impl PlanFilter {
    fn query(self) -> &'static str {
        match self {
            PlanFilter::Future => "&filter=future",
            PlanFilter::Past => "&filter=past",
            PlanFilter::All => "",
        }
    }
}

pub struct PcoClient<H: Http> {
    http: H,
    base: String,
    access_token: String,
    rate_limit: Mutex<Option<RateLimit>>,
}

impl<H: Http> std::fmt::Debug for PcoClient<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PcoClient")
            .field("base", &self.base)
            .field("access_token", &"<redacted>")
            .finish()
    }
}

impl<H: Http> PcoClient<H> {
    pub fn new(http: H, access_token: impl Into<String>) -> Self {
        Self {
            http,
            base: API_BASE.to_owned(),
            access_token: access_token.into(),
            rate_limit: Mutex::new(None),
        }
    }

    /// Point the client at another origin — a recorded fixture server in
    /// tests, and nothing else.
    pub fn with_base(mut self, base: impl Into<String>) -> Self {
        self.base = base.into();
        self
    }

    /// The transport underneath — so a caller that supplied one can still
    /// reach it (connection stats in the Lambda, the recording in tests).
    pub fn http(&self) -> &H {
        &self.http
    }

    /// What the last response said about the rate limit, if it said anything.
    pub fn rate_limit(&self) -> Option<RateLimit> {
        // A poisoned lock means another thread panicked mid-record; the rate
        // limit is advisory, so answering "unknown" beats propagating a panic
        // into a live service.
        self.rate_limit.lock().ok().and_then(|seen| *seen)
    }

    /// The organization's service types.
    pub async fn service_types(&self) -> Result<Vec<ServiceType>, Error> {
        let url = format!(
            "{}/services/v2/service_types?per_page={PER_PAGE}",
            self.base
        );
        let resources: Vec<Resource<ServiceTypeAttrs>> = self.list(url).await?;
        Ok(resources.into_iter().map(ServiceType::from).collect())
    }

    /// A service type's plans, oldest first within the filter.
    pub async fn plans(
        &self,
        service_type_id: &str,
        filter: PlanFilter,
    ) -> Result<Vec<Plan>, Error> {
        let url = format!(
            "{}/services/v2/service_types/{service_type_id}/plans?per_page={PER_PAGE}&order=sort_date{}",
            self.base,
            filter.query()
        );
        let resources: Vec<Resource<PlanAttrs>> = self.list(url).await?;
        Ok(resources.into_iter().map(Plan::from).collect())
    }

    /// The next plan on the calendar — "this Sunday", the one the bridge
    /// follows.
    pub async fn next_plan(&self, service_type_id: &str) -> Result<Option<Plan>, Error> {
        let url = format!(
            "{}/services/v2/service_types/{service_type_id}/plans?filter=future&order=sort_date&per_page=1",
            self.base
        );
        let document: Collection<PlanAttrs> = self.get(&url).await?;
        Ok(document.data.into_iter().next().map(Plan::from))
    }

    /// A plan's items, in plan order, every page of them.
    pub async fn items(
        &self,
        service_type_id: &str,
        plan_id: &str,
    ) -> Result<Vec<PlanItem>, Error> {
        let url = format!(
            "{}/services/v2/service_types/{service_type_id}/plans/{plan_id}/items?per_page={PER_PAGE}",
            self.base
        );
        let resources: Vec<Resource<ItemAttrs>> = self.list(url).await?;
        Ok(resources.into_iter().map(plan_item).collect())
    }

    /// One poll: where the service is, in a single request.
    ///
    /// The `include` asks Planning Center to sideload both ItemTimes, so the
    /// Live vertex's pointers resolve to plan items without a second hop. If a
    /// pointer comes back unresolved ([`LiveSnapshot::has_unresolved_pointer`])
    /// the association endpoint — [`PcoClient::live_item_time`] — is the
    /// fallback.
    pub async fn live(&self, service_type_id: &str, plan_id: &str) -> Result<LiveSnapshot, Error> {
        let url = format!(
            "{}/services/v2/service_types/{service_type_id}/plans/{plan_id}/live?include=current_item_time,next_item_time",
            self.base
        );
        let document: Single<LiveAttrs> = self.get(&url).await?;
        Ok(LiveSnapshot::from_document(document))
    }

    /// One of the Live vertex's ItemTime pointers, followed directly.
    ///
    /// The `include=item` carries the plan item's id along, so this answers the
    /// same question [`PcoClient::live`] does — at the cost of a second request
    /// per poll, which is why it is the fallback and not the path.
    pub async fn live_item_time(
        &self,
        service_type_id: &str,
        plan_id: &str,
        live_id: &str,
        slot: LiveSlot,
    ) -> Result<Option<ItemTime>, Error> {
        let url = format!(
            "{}/services/v2/service_types/{service_type_id}/plans/{plan_id}/live/{live_id}/{}?include=item",
            self.base,
            slot.as_str()
        );
        // Nothing there: the association answers with a null `data`.
        let document: MaybeSingle<ItemTimeAttrs> = self.get(&url).await?;
        Ok(document.data.map(ItemTime::from))
    }

    /// Follow `links.next` until the list runs out.
    async fn list<A>(&self, first: String) -> Result<Vec<Resource<A>>, Error>
    where
        A: DeserializeOwned + Default,
    {
        let mut url = Some(first);
        let mut out: Vec<Resource<A>> = Vec::new();
        for _ in 0..MAX_PAGES {
            let Some(next) = url.take() else { break };
            let document: Collection<A> = self.get(&next).await?;
            out.extend(document.data);
            url = document.links.next;
        }
        Ok(out)
    }

    async fn get<T: DeserializeOwned>(&self, url: &str) -> Result<T, Error> {
        let request = HttpRequest::get(url).with_bearer(&self.access_token);
        let response = self.http.send(request).await?;

        if let Ok(mut seen) = self.rate_limit.lock() {
            *seen = read_rate_limit(&response);
        }

        match response.status {
            200..=299 => {
                serde_json::from_str(&response.body).map_err(|e| Error::Decode(e.to_string()))
            }
            401 => Err(Error::Unauthorized),
            429 => Err(Error::RateLimited {
                retry_after_s: response
                    .header("Retry-After")
                    .and_then(|v| v.trim().parse().ok()),
            }),
            status => Err(Error::Status {
                status,
                detail: truncate(&response.body, 300),
            }),
        }
    }
}

fn read_rate_limit(response: &crate::http::HttpResponse) -> Option<RateLimit> {
    let limit = response
        .header("X-PCO-API-Request-Rate-Limit")?
        .trim()
        .parse()
        .ok()?;
    let count = response
        .header("X-PCO-API-Request-Rate-Count")?
        .trim()
        .parse()
        .ok()?;
    Some(RateLimit { limit, count })
}

fn truncate(body: &str, max: usize) -> String {
    match body.char_indices().nth(max) {
        Some((end, _)) => body[..end].to_owned(),
        None => body.to_owned(),
    }
}
