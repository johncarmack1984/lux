//! Planning Center Services, read-only.
//!
//! This crate does four things and refuses to do a fifth:
//!
//! - [`oauth`] — the authorization-code flow: build the consent URL, exchange
//!   the code, refresh the token. The one client secret lives here.
//! - [`jsonapi`] — the JSON:API envelope Planning Center answers in
//!   (`data`/`included`/`links`), typed once so no reader hand-rolls it.
//! - [`services`] — the vertices the bridge reads: service types, plans, plan
//!   items, and the Live vertex that says where the service is right now.
//! - [`client`] — [`PcoClient`], which issues those reads over any [`Http`]
//!   transport, so every one of them is testable against a recorded response.
//!
//! **The fifth thing it will not do is write.** Planning Center's `services`
//! scope has no read-only variant — the Live vertex's `go_to_next_item`,
//! `go_to_previous_item` and `toggle_control` actions sit inside the same
//! token the bridge already holds — so "lux never advances someone else's
//! service" cannot be enforced by the grant. It is enforced here instead:
//! [`PcoClient`] builds `GET` requests and nothing else, no method on it names
//! a write action, and the only `POST` in the crate is the token endpoint in
//! [`oauth`], which talks to the OAuth server rather than the API. A write
//! would have to be *written*, in a diff, on purpose.
//!
//! Rate limits are Planning Center's, not ours: 100 requests per 20 seconds
//! per connected church, reported on every response and re-read from the
//! headers rather than assumed ([`RateLimit`]). A two-second poll of one plan's
//! Live vertex spends ten of them.

pub mod client;
pub mod error;
pub mod http;
pub mod jsonapi;
pub mod oauth;
pub mod services;

pub use client::{PcoClient, PlanFilter, RateLimit, API_BASE};
pub use error::Error;
pub use http::{Http, HttpRequest, HttpResponse, Method, ReqwestHttp};
pub use oauth::{OAuthApp, Tokens};
pub use services::{LiveSlot, LiveSnapshot, Organization, Plan, ServiceType};
