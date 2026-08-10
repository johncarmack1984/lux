//! The Services vertices the bridge reads, and what it keeps of them.
//!
//! Four resources, in the order a connection uses them:
//!
//! - **ServiceType** — what the church calls a recurring service ("Sunday
//!   9:00"). The cue map hangs off this and nothing else.
//! - **Plan** — one week of it.
//! - **Item** — a row of the plan. Reduced immediately to
//!   [`lux_cue::PlanItem`], because the only questions a light has about a row
//!   are its title, its type, and which library song it is.
//! - **Live** — where the service is *right now*: a pointer at an ItemTime,
//!   which in turn points at an Item. That double hop is why a poll asks for
//!   `?include=current_item_time,next_item_time`.
//!
//! The attribute structs below hold what the API documents and this bridge
//! reads; everything else in a response is ignored on purpose. `Option`
//! everywhere is not defensiveness for its own sake — a plan with no title, a
//! live vertex with nothing live, and an item with no song are all ordinary
//! Sunday states.

use lux_cue::{LivePosition, PlanItem};
use serde::Deserialize;

use crate::jsonapi::{included_as, Resource, Single};

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ServiceTypeAttrs {
    pub name: Option<String>,
    pub frequency: Option<String>,
    pub sequence: Option<i64>,
    pub archived_at: Option<String>,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PlanAttrs {
    pub title: Option<String>,
    pub dates: Option<String>,
    pub short_dates: Option<String>,
    pub series_title: Option<String>,
    pub sort_date: Option<String>,
    pub items_count: Option<i64>,
    pub total_length: Option<i64>,
    pub public: Option<bool>,
    pub planning_center_url: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ItemAttrs {
    pub title: Option<String>,
    pub sequence: Option<i64>,
    /// `song`, `header`, `media`, `item`, or an organization's custom type.
    pub item_type: Option<String>,
    /// Planned length in seconds.
    pub length: Option<i64>,
    pub description: Option<String>,
    pub key_name: Option<String>,
    pub service_position: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ItemTimeAttrs {
    pub exclude: Option<bool>,
    pub length: Option<i64>,
    pub length_offset: Option<i64>,
    pub live_start_at: Option<String>,
    pub live_end_at: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LiveAttrs {
    pub title: Option<String>,
    pub series_title: Option<String>,
    pub dates: Option<String>,
    /// Whether this connection *could* drive the service in Planning Center.
    /// Read and shown; never acted on.
    pub can_control: Option<bool>,
    pub can_take_control: Option<bool>,
    pub can_chat: Option<bool>,
    pub live_channel: Option<String>,
    pub chat_room_channel: Option<String>,
}

/// The connected church, as the Services API root names it.
///
/// Read once at connect, to answer "which organization is this token for?" —
/// the question `lux_wire::plan::PlanBinding` carries an `org_id` to settle.
/// Everything here is optional because identifying the church is a *nicety*:
/// a connection whose org could not be named still reads plans perfectly well,
/// so a surprise in this document degrades the label, never the connection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Organization {
    pub id: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct OrganizationAttrs {
    pub name: Option<String>,
}

impl From<Resource<OrganizationAttrs>> for Organization {
    fn from(resource: Resource<OrganizationAttrs>) -> Self {
        Self {
            id: (!resource.id.is_empty()).then_some(resource.id),
            name: resource.attributes.name,
        }
    }
}

/// A recurring service. The cue map's anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceType {
    pub id: String,
    pub name: Option<String>,
    pub frequency: Option<String>,
    /// Archived or deleted service types are still returned by the API; a
    /// church should not be offered one to bind a setup to.
    pub retired: bool,
}

impl From<Resource<ServiceTypeAttrs>> for ServiceType {
    fn from(resource: Resource<ServiceTypeAttrs>) -> Self {
        let retired =
            resource.attributes.archived_at.is_some() || resource.attributes.deleted_at.is_some();
        Self {
            id: resource.id,
            name: resource.attributes.name,
            frequency: resource.attributes.frequency,
            retired,
        }
    }
}

/// One week's plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub id: String,
    /// Plans are often untitled; `dates` is what the church actually reads.
    pub title: Option<String>,
    pub series_title: Option<String>,
    pub dates: Option<String>,
    pub short_dates: Option<String>,
    /// RFC 3339, and the field the API orders plans by.
    pub sort_date: Option<String>,
    pub items_count: Option<i64>,
    pub planning_center_url: Option<String>,
}

impl From<Resource<PlanAttrs>> for Plan {
    fn from(resource: Resource<PlanAttrs>) -> Self {
        Self {
            id: resource.id,
            title: resource.attributes.title,
            series_title: resource.attributes.series_title,
            dates: resource.attributes.dates,
            short_dates: resource.attributes.short_dates,
            sort_date: resource.attributes.sort_date,
            items_count: resource.attributes.items_count,
            planning_center_url: resource.attributes.planning_center_url,
        }
    }
}

/// One plan row, as a cue map sees it.
///
/// An item with no title becomes an empty title rather than being dropped: the
/// plan has a row there, the surface must show it, and a title rule simply
/// won't match it.
pub fn plan_item(resource: Resource<ItemAttrs>) -> PlanItem {
    let song_id = resource.related_id("song").map(str::to_owned);
    PlanItem {
        id: resource.id,
        sequence: resource.attributes.sequence.unwrap_or_default(),
        title: resource.attributes.title.unwrap_or_default(),
        item_type: resource.attributes.item_type.unwrap_or_default(),
        song_id,
        length_s: resource.attributes.length,
    }
}

/// One item's slot in the running order of a live service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemTime {
    pub id: String,
    /// The plan item this slot belongs to.
    pub item_id: Option<String>,
    pub live_start_at: Option<String>,
    pub live_end_at: Option<String>,
    pub length_s: Option<i64>,
}

impl From<Resource<ItemTimeAttrs>> for ItemTime {
    fn from(resource: Resource<ItemTimeAttrs>) -> Self {
        let item_id = resource.related_id("item").map(str::to_owned);
        Self {
            id: resource.id,
            item_id,
            live_start_at: resource.attributes.live_start_at,
            live_end_at: resource.attributes.live_end_at,
            length_s: resource.attributes.length,
        }
    }
}

/// Which of the Live vertex's two item pointers to follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveSlot {
    Current,
    Next,
}

impl LiveSlot {
    /// The relationship name, which is also the association path segment.
    pub fn as_str(self) -> &'static str {
        match self {
            LiveSlot::Current => "current_item_time",
            LiveSlot::Next => "next_item_time",
        }
    }
}

/// One poll of the Live vertex, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSnapshot {
    pub id: String,
    pub title: Option<String>,
    pub series_title: Option<String>,
    /// Display only. lux reads the service; it never advances it.
    pub can_control: bool,
    /// The ItemTime the service is on, when it is live.
    pub current: Option<ItemTime>,
    pub next: Option<ItemTime>,
    /// The pointer ids as the Live vertex gave them, whether or not the
    /// sideloaded ItemTimes came back with them. A `current_item_time_id` with
    /// no [`LiveSnapshot::current`] means the server didn't honour the
    /// `include`, and the association endpoint is the way to resolve it.
    pub current_item_time_id: Option<String>,
    pub next_item_time_id: Option<String>,
}

impl LiveSnapshot {
    /// Read a `GET …/live?include=current_item_time,next_item_time` response.
    pub fn from_document(document: Single<LiveAttrs>) -> Self {
        let current_item_time_id = document
            .data
            .related_id(LiveSlot::Current.as_str())
            .map(str::to_owned);
        let next_item_time_id = document
            .data
            .related_id(LiveSlot::Next.as_str())
            .map(str::to_owned);
        let sideloaded = |id: &Option<String>| -> Option<ItemTime> {
            let id = id.as_deref()?;
            included_as::<ItemTimeAttrs>(&document.included, "ItemTime", id).map(ItemTime::from)
        };
        Self {
            current: sideloaded(&current_item_time_id),
            next: sideloaded(&next_item_time_id),
            id: document.data.id.clone(),
            title: document.data.attributes.title.clone(),
            series_title: document.data.attributes.series_title.clone(),
            can_control: document.data.attributes.can_control.unwrap_or(false),
            current_item_time_id,
            next_item_time_id,
        }
    }

    /// The plan item the service is on, when the response resolved it.
    pub fn current_item_id(&self) -> Option<&str> {
        self.current.as_ref()?.item_id.as_deref()
    }

    pub fn next_item_id(&self) -> Option<&str> {
        self.next.as_ref()?.item_id.as_deref()
    }

    /// What the follow engine consumes.
    pub fn position(&self) -> LivePosition {
        LivePosition {
            current_item_id: self.current_item_id().map(str::to_owned),
            next_item_id: self.next_item_id().map(str::to_owned),
            can_control: self.can_control,
        }
    }

    /// Whether the Live vertex pointed at an ItemTime we could not resolve —
    /// the one case where the single-request poll needs the association
    /// endpoint as a second hop.
    pub fn has_unresolved_pointer(&self) -> bool {
        (self.current_item_time_id.is_some() && self.current.is_none())
            || (self.next_item_time_id.is_some() && self.next.is_none())
    }
}
