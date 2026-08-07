//! Cues: the service plan turned into a scene list, and the decision of which
//! scene to fire while the plan is live.
//!
//! Two halves, both pure:
//!
//! - [`resolve`] — a [`lux_wire::plan::CueMap`] plus this week's [`PlanItem`]s
//!   gives a [`CueSheet`]: one cue per item, each naming the scene it calls
//!   for and which rule chose it. The map hangs off the *service type*, so the
//!   same map resolves next week's plan without anyone touching it.
//! - [`follow`] — a [`Follower`] takes polled live positions and answers one
//!   question per observation: *fire a scene, or hold?* It owns no clock, no
//!   socket and no lights, so the whole of it is testable against recorded
//!   Planning Center responses, which is the only way a "what happens when the
//!   network dies mid-service" rule can be trusted before it is needed.
//!
//! Nothing here talks to Planning Center. `lux-pco` fetches and converts;
//! this crate decides; the caller recalls the scene through the ordinary
//! scene-recall path (`lux_engine::fade`), so a bridge-fired look crossfades
//! exactly like a hand-pressed one.

pub mod follow;
pub mod resolve;

pub use follow::{Decision, Follower, LivePosition, Mode, Observation, Outcome, Status};
pub use resolve::{Cue, CueSheet, CueSource};

use serde::{Deserialize, Serialize};

/// One item of a service plan, reduced to what a cue map can match on.
///
/// This is `lux-pco`'s output and this crate's input — the Planning Center
/// `Item` vertex minus everything a lighting decision has no business reading
/// (arrangements, keys, attachments, notes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    /// The Planning Center item id. **Unique to this plan** — a new plan mints
    /// new ids for the same songs, which is why a cue map never stores one.
    pub id: String,
    /// Position in the plan. The plan's own order, not ours.
    pub sequence: i64,
    pub title: String,
    /// `song`, `header`, `media`, `item`, or an organization's custom type.
    pub item_type: String,
    /// The library song behind a `song` item, when there is one. The only
    /// identifier here that survives into next week's plan, and therefore the
    /// only thing a pin may be keyed on.
    #[serde(default)]
    pub song_id: Option<String>,
    /// Planned length in seconds, when the plan gives one. Display only — the
    /// follow engine fires on the live position, never on a schedule.
    #[serde(default)]
    pub length_s: Option<i64>,
}

impl PlanItem {
    /// A plain item with no song and no length — the shape most non-song rows
    /// have, and the one tests build.
    pub fn new(
        id: impl Into<String>,
        sequence: i64,
        title: impl Into<String>,
        item_type: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            sequence,
            title: title.into(),
            item_type: item_type.into(),
            song_id: None,
            length_s: None,
        }
    }

    pub fn with_song(mut self, song_id: impl Into<String>) -> Self {
        self.song_id = Some(song_id.into());
        self
    }

    pub fn with_length_s(mut self, length_s: i64) -> Self {
        self.length_s = Some(length_s);
        self
    }
}
