//! Following the live plan: what to fire, and when not to.
//!
//! A [`Follower`] is fed one observation per poll of Planning Center's Live
//! vertex and answers with a [`Decision`]. It holds no clock, no socket and no
//! lights — the caller polls, the caller recalls the scene — which is what
//! makes the rules below testable before a real Sunday tests them.
//!
//! The rules, in the order they override each other:
//!
//! 1. **Manual Go always wins.** A Go fires immediately and *latches* over the
//!    current item: follow stops firing until the plan moves to a different
//!    item, then resumes on its own. An operator who reaches for the button
//!    has decided something the plan doesn't know; automation may not argue
//!    with them, and it may not sulk either.
//! 2. **Failure holds.** A failed poll never changes a light — not to a
//!    fallback, not to black. After [`CONNECTION_LOST_AFTER_MS`] the status
//!    turns [`Status::Lost`] so the surface can say *"Not following — plan
//!    connection lost"*, and the rig keeps the last scene until a human or the
//!    network says otherwise. This is lux's founding law ("live DMX output
//!    never depends on the network") applied to the bridge.
//! 3. **The live position is the truth, not the transition.** Every decision
//!    asks "what is current?", never "what came next?" — so joining a service
//!    already in progress fires the item that is up, and an item skipped in
//!    the room is skipped here too, with nothing replayed on the way past.
//! 4. **Fire only on a change.** A scene already on the rig is not re-fired,
//!    so two items sharing a look don't restart the crossfade between them.
//! 5. **An unknown item asks, once.** If the live item isn't in the cue sheet,
//!    the plan was edited after we pulled it: the follower answers
//!    [`Outcome::RefreshPlan`] once and then holds, rather than begging every
//!    two seconds for a plan that may genuinely no longer contain it.
//!
//! lux never writes to Planning Center — no `go_to_next_item`, no
//! `toggle_control`. [`LivePosition::can_control`] is carried for display
//! only; nothing in this crate, or in `lux-pco`, can act on it.

use crate::{CueSheet, PlanItem};

/// How often the caller should poll the Live vertex while a plan is live.
///
/// One request every two seconds is 10 per 20-second window against Planning
/// Center's documented 100-per-20-seconds-per-user ceiling — ten times the
/// headroom, for the one church this connection belongs to. The poller must
/// still read the rate-limit headers (`lux_pco::RateLimit`) rather than trust
/// this number: the documented limits may be adjusted without notice.
pub const POLL_INTERVAL_MS: u64 = 2_000;

/// How long the Live vertex may go unheard before the surface must say the
/// connection is gone. Measured from the last *successful* poll, not from the
/// first failure — what matters is how stale the follower's picture of the
/// service is. Five missed polls: long enough to ride out a flaky sanctuary
/// Wi-Fi blip without alarming a volunteer, short enough that a real outage is
/// visible before the next cue would have been.
pub const CONNECTION_LOST_AFTER_MS: i64 = 10_000;

/// Where the plan is right now, as the Live vertex reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LivePosition {
    /// The item the service is on, or `None` before it starts and after it
    /// ends. `None` holds the last scene — the end of a service is not a
    /// reason to change the lights.
    pub current_item_id: Option<String>,
    /// The item after it, for the surface's "next: Sermon" preview.
    pub next_item_id: Option<String>,
    /// Whether this connection *could* control the service in Planning Center.
    /// Display only: lux reads the plan and never advances it.
    pub can_control: bool,
}

impl LivePosition {
    /// The position of a service sitting on one item, with nothing queued.
    pub fn on(item_id: impl Into<String>) -> Self {
        Self {
            current_item_id: Some(item_id.into()),
            next_item_id: None,
            can_control: false,
        }
    }

    pub fn then(mut self, next_item_id: impl Into<String>) -> Self {
        self.next_item_id = Some(next_item_id.into());
        self
    }

    /// Nothing is live: before the service, or after it ended.
    pub fn idle() -> Self {
        Self::default()
    }
}

/// One poll's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observation {
    /// The Live vertex answered.
    Live(LivePosition),
    /// The poll failed — timeout, 5xx, rate limit, expired token, anything.
    /// The follower does not care which: every failure holds.
    PollFailed,
}

/// What the caller should do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Change nothing.
    Hold,
    /// Recall this scene, with its own fade.
    Fire { scene_id: String },
    /// The live item isn't in the cue sheet — re-pull the plan's items and
    /// [`Follower::retarget`]. Asked at most once per item.
    RefreshPlan,
}

/// What the surface should say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Manual mode: only Go fires.
    Manual,
    /// Following, but nothing is live yet (or the service has ended).
    Idle,
    Following {
        item_id: String,
    },
    /// Following, but the operator's Go owns the rig until the plan moves on.
    Overridden,
    /// Polls have been failing for longer than [`CONNECTION_LOST_AFTER_MS`].
    Lost,
}

/// An outcome and the status that goes with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub outcome: Outcome,
    pub status: Status,
}

/// Whether the plan drives the rig.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Follow,
    /// Nothing fires but Go. The mode the room falls back to when the plan is
    /// wrong, the network is gone, or a volunteer simply wants the wheel.
    Manual,
}

/// The follow-live state machine.
#[derive(Debug, Clone)]
pub struct Follower {
    sheet: CueSheet,
    mode: Mode,
    /// The live item as of the last successful poll.
    current: Option<String>,
    /// The item Planning Center says is next, for the preview.
    next: Option<String>,
    /// The scene we last put on the rig, so we don't fire it twice.
    fired: Option<String>,
    /// Set by a manual Go; holds the item it was pressed during. Cleared when
    /// the plan moves to a different item.
    latched_at: Option<Option<String>>,
    /// Manual mode's cursor into the cue sheet, so Go walks the plan.
    cursor: Option<usize>,
    /// When the Live vertex last answered.
    last_contact_ms: Option<i64>,
    /// Start of the current run of failed polls, for a connection that has
    /// never once succeeded.
    failing_since: Option<i64>,
    lost: bool,
    /// The item we already asked to refresh the plan for.
    refresh_asked_for: Option<String>,
}

impl Follower {
    pub fn new(sheet: CueSheet) -> Self {
        Self {
            sheet,
            mode: Mode::default(),
            current: None,
            next: None,
            fired: None,
            latched_at: None,
            cursor: None,
            last_contact_ms: None,
            failing_since: None,
            lost: false,
            refresh_asked_for: None,
        }
    }

    /// Tell the follower which scene is already on the rig — so a follower
    /// built mid-service doesn't re-fire the look that is up.
    pub fn with_scene_on_rig(mut self, scene_id: impl Into<String>) -> Self {
        self.fired = Some(scene_id.into());
        self
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn sheet(&self) -> &CueSheet {
        &self.sheet
    }

    /// The scene the follower believes is on the rig.
    pub fn scene_on_rig(&self) -> Option<&str> {
        self.fired.as_deref()
    }

    pub fn current_item(&self) -> Option<&PlanItem> {
        let id = self.current.as_deref()?;
        self.sheet.cue_for(id).map(|c| &c.item)
    }

    /// The item Planning Center says is next, if we hold it — the "next:
    /// Sermon" preview on the surface.
    pub fn next_item(&self) -> Option<&PlanItem> {
        let id = self.next.as_deref()?;
        self.sheet.cue_for(id).map(|c| &c.item)
    }

    pub fn status(&self) -> Status {
        if self.mode == Mode::Manual {
            return Status::Manual;
        }
        if self.lost {
            return Status::Lost;
        }
        if self.latched_at.is_some() {
            return Status::Overridden;
        }
        match &self.current {
            Some(item_id) => Status::Following {
                item_id: item_id.clone(),
            },
            None => Status::Idle,
        }
    }

    /// Switch between following the plan and driving by hand.
    ///
    /// Nothing fires on the toggle itself: returning to Follow re-syncs on the
    /// next poll (within [`POLL_INTERVAL_MS`]), which is a beat later and
    /// never a surprise in the middle of a sentence.
    pub fn set_mode(&mut self, mode: Mode) -> Decision {
        self.mode = mode;
        if mode == Mode::Follow {
            self.latched_at = None;
        }
        self.decide(Outcome::Hold)
    }

    /// Take a fresh cue sheet — a re-pulled plan, or an edited cue map — and
    /// re-decide against it immediately. A cue whose scene changed under the
    /// live item takes effect now, not at the next item.
    pub fn retarget(&mut self, sheet: CueSheet) -> Decision {
        self.sheet = sheet;
        self.cursor = None;
        self.evaluate()
    }

    /// Feed one poll's result.
    pub fn observe(&mut self, observation: Observation, now_ms: i64) -> Decision {
        match observation {
            Observation::PollFailed => {
                let quiet_since = match self.last_contact_ms {
                    Some(at) => at,
                    None => *self.failing_since.get_or_insert(now_ms),
                };
                if now_ms.saturating_sub(quiet_since) >= CONNECTION_LOST_AFTER_MS {
                    self.lost = true;
                }
                // Never a scene change. Not a fallback, not a blackout.
                self.decide(Outcome::Hold)
            }
            Observation::Live(position) => {
                self.last_contact_ms = Some(now_ms);
                self.failing_since = None;
                self.lost = false;
                self.next = position.next_item_id;
                if self.current != position.current_item_id {
                    // The plan moved: the operator's override expires with the
                    // item it was made during, and a new item may ask again.
                    self.latched_at = None;
                    self.refresh_asked_for = None;
                    self.current = position.current_item_id;
                    self.cursor = None;
                }
                self.evaluate()
            }
        }
    }

    /// Manual Go: put this scene up now, whatever the plan thinks.
    ///
    /// Fires even when the scene is already on the rig — the operator pressed
    /// the button, and re-running the fade is a legitimate thing to want.
    pub fn go(&mut self, scene_id: impl Into<String>) -> Decision {
        let scene_id = scene_id.into();
        self.fired = Some(scene_id.clone());
        self.latched_at = Some(self.current.clone());
        self.cursor = self
            .current
            .as_deref()
            .and_then(|id| self.sheet.index_of(id))
            .or(self.cursor);
        self.decide(Outcome::Fire { scene_id })
    }

    /// Manual Go down the cue sheet: fire the next cue after wherever the
    /// operator (or the plan) last was.
    ///
    /// Walking onto an unmapped item advances the cursor and holds — the plan
    /// has a row there, the map has nothing to say about it, and pressing Go
    /// again moves on.
    pub fn go_next(&mut self) -> Decision {
        let from = self
            .cursor
            .or_else(|| {
                self.current
                    .as_deref()
                    .and_then(|id| self.sheet.index_of(id))
            })
            .map(|i| i.saturating_add(1))
            .unwrap_or(0);
        let Some(cue) = self.sheet.get(from) else {
            return self.decide(Outcome::Hold); // the end of the plan
        };
        self.cursor = Some(from);
        self.latched_at = Some(self.current.clone());
        match cue.scene_id.clone() {
            Some(scene_id) => {
                self.fired = Some(scene_id.clone());
                self.decide(Outcome::Fire { scene_id })
            }
            None => self.decide(Outcome::Hold),
        }
    }

    /// The rule ladder, applied to the current state.
    fn evaluate(&mut self) -> Decision {
        if self.mode == Mode::Manual || self.latched_at.is_some() {
            return self.decide(Outcome::Hold);
        }
        let Some(item_id) = self.current.clone() else {
            return self.decide(Outcome::Hold); // not live: hold what's up
        };
        if !self.sheet.contains(&item_id) {
            if self.refresh_asked_for.as_deref() == Some(item_id.as_str()) {
                return self.decide(Outcome::Hold);
            }
            self.refresh_asked_for = Some(item_id);
            return self.decide(Outcome::RefreshPlan);
        }
        let target = self.sheet.scene_for(&item_id).map(str::to_owned);
        match target {
            Some(scene_id) if Some(scene_id.as_str()) != self.fired.as_deref() => {
                self.fired = Some(scene_id.clone());
                self.decide(Outcome::Fire { scene_id })
            }
            // Unmapped, or already on the rig.
            _ => self.decide(Outcome::Hold),
        }
    }

    fn decide(&self, outcome: Outcome) -> Decision {
        Decision {
            outcome,
            status: self.status(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::CueSheet;
    use crate::PlanItem;
    use lux_wire::plan::{CueMap, CueRule, TitleMode};

    fn sheet() -> CueSheet {
        let map = CueMap::new(
            "st-1".into(),
            vec![
                CueRule::ItemType {
                    item_type: "song".into(),
                    scene_id: "worship".into(),
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
        .with_fallback("house".into());
        CueSheet::resolve(
            &map,
            &[
                PlanItem::new("i1", 1, "Pre-Service", "header"),
                PlanItem::new("i2", 2, "Great Are You Lord", "song").with_song("1001"),
                PlanItem::new("i3", 3, "King of Kings", "song").with_song("1002"),
                PlanItem::new("i4", 4, "Sermon — Part 3", "header"),
                PlanItem::new("i5", 5, "Doxology", "song").with_song("1003"),
                PlanItem::new("i6", 6, "Dismissal", "item"),
            ],
        )
    }

    fn fired(decision: &Decision) -> Option<&str> {
        match &decision.outcome {
            Outcome::Fire { scene_id } => Some(scene_id),
            _ => None,
        }
    }

    /// Drive a follower through a sequence of live items and collect what it
    /// fired, in order — the shape almost every test below wants.
    fn run(follower: &mut Follower, items: &[&str], start_ms: i64) -> Vec<String> {
        let mut out = Vec::new();
        for (tick, item) in items.iter().enumerate() {
            let at = start_ms + i64::try_from(tick).unwrap_or(0) * 2_000;
            let decision = follower.observe(Observation::Live(LivePosition::on(*item)), at);
            if let Outcome::Fire { scene_id } = decision.outcome {
                out.push(scene_id);
            }
        }
        out
    }

    #[test]
    fn a_whole_service_fires_each_scene_once() {
        let mut follower = Follower::new(sheet());
        // Two polls per item: the second must not re-fire.
        let polls = [
            "i1", "i1", "i2", "i2", "i3", "i3", "i4", "i4", "i5", "i5", "i6", "i6",
        ];
        assert_eq!(
            run(&mut follower, &polls, 0),
            // i2 -> i3 are both songs and share "worship": one fire, no
            // restarted crossfade between them.
            vec!["house", "worship", "sermon", "doxology", "house"]
        );
    }

    #[test]
    fn joining_mid_service_fires_the_item_that_is_up() {
        // The volunteer opened lux during the sermon. Nothing "transitioned",
        // and the right answer is still the sermon look.
        let mut follower = Follower::new(sheet());
        let decision = follower.observe(Observation::Live(LivePosition::on("i4")), 0);
        assert_eq!(fired(&decision), Some("sermon"));
        assert_eq!(
            decision.status,
            Status::Following {
                item_id: "i4".into()
            }
        );
    }

    #[test]
    fn a_skipped_item_is_skipped_not_replayed() {
        // The room jumped from the first song straight to the sermon; the
        // scenes in between never happened.
        let mut follower = Follower::new(sheet());
        assert_eq!(
            run(&mut follower, &["i2", "i4"], 0),
            vec!["worship", "sermon"]
        );
    }

    #[test]
    fn a_follower_told_what_is_on_the_rig_does_not_re_fire_it() {
        let mut follower = Follower::new(sheet()).with_scene_on_rig("sermon");
        let decision = follower.observe(Observation::Live(LivePosition::on("i4")), 0);
        assert_eq!(decision.outcome, Outcome::Hold);
    }

    #[test]
    fn manual_go_wins_and_holds_until_the_plan_moves_on() {
        let mut follower = Follower::new(sheet());
        follower.observe(Observation::Live(LivePosition::on("i2")), 0);

        // The pastor asked for house lights during the song.
        let decision = follower.go("house");
        assert_eq!(fired(&decision), Some("house"));
        assert_eq!(decision.status, Status::Overridden);

        // Follow does not argue while the plan is still on that item.
        let decision = follower.observe(Observation::Live(LivePosition::on("i2")), 2_000);
        assert_eq!(decision.outcome, Outcome::Hold);
        assert_eq!(decision.status, Status::Overridden);

        // The plan moves: automation resumes on its own, no re-arming.
        let decision = follower.observe(Observation::Live(LivePosition::on("i4")), 4_000);
        assert_eq!(fired(&decision), Some("sermon"));
        assert_eq!(
            decision.status,
            Status::Following {
                item_id: "i4".into()
            }
        );
    }

    #[test]
    fn manual_go_fires_even_when_the_scene_is_already_up() {
        let mut follower = Follower::new(sheet());
        follower.observe(Observation::Live(LivePosition::on("i4")), 0);
        let decision = follower.go("sermon");
        assert_eq!(fired(&decision), Some("sermon"));
    }

    #[test]
    fn manual_mode_fires_nothing_but_go() {
        let mut follower = Follower::new(sheet());
        assert_eq!(follower.set_mode(Mode::Manual).status, Status::Manual);

        let decision = follower.observe(Observation::Live(LivePosition::on("i2")), 0);
        assert_eq!(decision.outcome, Outcome::Hold);
        assert_eq!(decision.status, Status::Manual);

        // Go still works with no connection to anything.
        assert_eq!(fired(&follower.go("worship")), Some("worship"));

        // Back to Follow: nothing fires on the toggle, the next poll re-syncs.
        let decision = follower.set_mode(Mode::Follow);
        assert_eq!(decision.outcome, Outcome::Hold);
        let decision = follower.observe(Observation::Live(LivePosition::on("i4")), 2_000);
        assert_eq!(fired(&decision), Some("sermon"));
    }

    #[test]
    fn go_next_walks_the_plan_by_hand() {
        let mut follower = Follower::new(sheet());
        follower.set_mode(Mode::Manual);
        assert_eq!(fired(&follower.go_next()), Some("house")); // i1
        assert_eq!(fired(&follower.go_next()), Some("worship")); // i2
                                                                 // i3 is another song: pressing Go re-runs the same look's fade rather
                                                                 // than swallowing the press, because the operator pressed it.
        assert_eq!(fired(&follower.go_next()), Some("worship"));
    }

    #[test]
    fn go_next_continues_from_where_the_plan_was() {
        let mut follower = Follower::new(sheet());
        follower.observe(Observation::Live(LivePosition::on("i4")), 0);
        follower.set_mode(Mode::Manual);
        // The plan died mid-sermon; the next thing by hand is the Doxology.
        assert_eq!(fired(&follower.go_next()), Some("doxology"));
        assert_eq!(fired(&follower.go_next()), Some("house")); // i6
                                                               // The end of the plan is a hold, not a wrap-around.
        assert_eq!(follower.go_next().outcome, Outcome::Hold);
    }

    #[test]
    fn network_loss_holds_the_last_scene_and_then_says_so() {
        let mut follower = Follower::new(sheet());
        assert_eq!(
            fired(&follower.observe(Observation::Live(LivePosition::on("i4")), 0)),
            Some("sermon")
        );

        // Four failed polls inside the grace window: hold, and don't alarm.
        for tick in 1..=4 {
            let decision = follower.observe(Observation::PollFailed, tick * 2_000);
            assert_eq!(decision.outcome, Outcome::Hold);
            assert_eq!(
                decision.status,
                Status::Following {
                    item_id: "i4".into()
                }
            );
        }
        // Ten seconds of silence: the surface must say the connection is gone.
        let decision = follower.observe(Observation::PollFailed, 10_000);
        assert_eq!(decision.outcome, Outcome::Hold);
        assert_eq!(decision.status, Status::Lost);
        assert_eq!(follower.scene_on_rig(), Some("sermon"));

        // It comes back, and the service has moved on: catch up in one fire.
        let decision = follower.observe(Observation::Live(LivePosition::on("i5")), 30_000);
        assert_eq!(fired(&decision), Some("doxology"));
    }

    #[test]
    fn recovering_onto_the_same_item_changes_nothing() {
        let mut follower = Follower::new(sheet());
        follower.observe(Observation::Live(LivePosition::on("i4")), 0);
        follower.observe(Observation::PollFailed, 20_000);
        assert_eq!(follower.status(), Status::Lost);
        let decision = follower.observe(Observation::Live(LivePosition::on("i4")), 22_000);
        assert_eq!(decision.outcome, Outcome::Hold);
        assert_eq!(
            decision.status,
            Status::Following {
                item_id: "i4".into()
            }
        );
    }

    #[test]
    fn a_failure_run_restarts_after_a_good_poll() {
        let mut follower = Follower::new(sheet());
        follower.observe(Observation::PollFailed, 0);
        follower.observe(Observation::PollFailed, 8_000);
        follower.observe(Observation::Live(LivePosition::on("i2")), 9_000);
        // The old streak is gone: a single later failure must not trip Lost.
        let decision = follower.observe(Observation::PollFailed, 11_000);
        assert_eq!(
            decision.status,
            Status::Following {
                item_id: "i2".into()
            }
        );
    }

    #[test]
    fn nothing_live_holds_rather_than_blacking_out() {
        let mut follower = Follower::new(sheet());
        follower.observe(Observation::Live(LivePosition::on("i5")), 0);
        // The service ended: current_item_time goes null.
        let decision = follower.observe(Observation::Live(LivePosition::idle()), 2_000);
        assert_eq!(decision.outcome, Outcome::Hold);
        assert_eq!(decision.status, Status::Idle);
        assert_eq!(follower.scene_on_rig(), Some("doxology"));
    }

    #[test]
    fn an_unmapped_item_holds() {
        // A map with no fallback: the plan's own row for "Offering" says
        // nothing about lights, so nothing about the lights changes.
        let map = CueMap::new(
            "st-1".into(),
            vec![CueRule::ItemType {
                item_type: "song".into(),
                scene_id: "worship".into(),
            }],
        );
        let sheet = CueSheet::resolve(
            &map,
            &[
                PlanItem::new("i1", 1, "Opener", "song").with_song("1"),
                PlanItem::new("i2", 2, "Offering", "item"),
            ],
        );
        let mut follower = Follower::new(sheet);
        assert_eq!(
            fired(&follower.observe(Observation::Live(LivePosition::on("i1")), 0)),
            Some("worship")
        );
        let decision = follower.observe(Observation::Live(LivePosition::on("i2")), 2_000);
        assert_eq!(decision.outcome, Outcome::Hold);
        assert_eq!(follower.scene_on_rig(), Some("worship"));
    }

    #[test]
    fn an_item_added_mid_service_asks_for_the_plan_once_then_holds() {
        let mut follower = Follower::new(sheet());
        follower.observe(Observation::Live(LivePosition::on("i4")), 0);

        // Someone inserted a baptism into the plan five minutes ago.
        let decision = follower.observe(Observation::Live(LivePosition::on("new-item")), 2_000);
        assert_eq!(decision.outcome, Outcome::RefreshPlan);

        // Until the caller re-pulls, the follower stops asking — a plan we
        // cannot see is not a reason to hammer the API every two seconds.
        let decision = follower.observe(Observation::Live(LivePosition::on("new-item")), 4_000);
        assert_eq!(decision.outcome, Outcome::Hold);
        assert_eq!(follower.scene_on_rig(), Some("sermon"));
    }

    #[test]
    fn retargeting_after_an_edit_fires_the_new_cue_now() {
        let mut follower = Follower::new(sheet());
        follower.observe(Observation::Live(LivePosition::on("i4")), 0);

        // The plan gained the item, and the map already had a rule for it.
        let map = CueMap::new(
            "st-1".into(),
            vec![CueRule::Title {
                pattern: "baptism".into(),
                mode: TitleMode::Contains,
                scene_id: "baptism".into(),
            }],
        );
        let edited = CueSheet::resolve(
            &map,
            &[
                PlanItem::new("i4", 4, "Sermon — Part 3", "header"),
                PlanItem::new("new-item", 5, "Baptism", "item"),
            ],
        );
        follower.observe(Observation::Live(LivePosition::on("new-item")), 2_000);
        let decision = follower.retarget(edited);
        assert_eq!(fired(&decision), Some("baptism"));
    }

    #[test]
    fn retargeting_a_changed_map_re_lights_the_current_item() {
        // The operator fixed the map while the sermon was on screen. The fix
        // is meant to be visible now, not next week.
        let mut follower = Follower::new(sheet());
        follower.observe(Observation::Live(LivePosition::on("i4")), 0);

        let map = CueMap::new(
            "st-1".into(),
            vec![CueRule::Title {
                pattern: "sermon".into(),
                mode: TitleMode::Contains,
                scene_id: "sermon-warmer".into(),
            }],
        );
        let edited =
            CueSheet::resolve(&map, &[PlanItem::new("i4", 4, "Sermon — Part 3", "header")]);
        assert_eq!(fired(&follower.retarget(edited)), Some("sermon-warmer"));
    }

    #[test]
    fn retargeting_does_not_overrule_a_manual_go() {
        let mut follower = Follower::new(sheet());
        follower.observe(Observation::Live(LivePosition::on("i4")), 0);
        follower.go("house");
        let decision = follower.retarget(sheet());
        assert_eq!(decision.outcome, Outcome::Hold);
        assert_eq!(follower.scene_on_rig(), Some("house"));
    }

    #[test]
    fn an_item_that_vanished_from_the_plan_holds() {
        let mut follower = Follower::new(sheet());
        follower.observe(Observation::Live(LivePosition::on("i4")), 0);
        follower.observe(Observation::Live(LivePosition::on("i5")), 2_000);

        // Re-pulled: i5 was deleted, and the live vertex hasn't caught up.
        let map = CueMap::new("st-1".into(), vec![]).with_fallback("house".into());
        let shrunk = CueSheet::resolve(&map, &[PlanItem::new("i4", 4, "Sermon", "header")]);
        assert_eq!(follower.retarget(shrunk).outcome, Outcome::RefreshPlan);
        // Asked once; then it holds the Doxology look rather than guessing.
        assert_eq!(
            follower
                .observe(Observation::Live(LivePosition::on("i5")), 4_000)
                .outcome,
            Outcome::Hold
        );
        assert_eq!(follower.scene_on_rig(), Some("doxology"));
    }

    #[test]
    fn the_surface_can_read_the_current_and_next_item() {
        let mut follower = Follower::new(sheet());
        follower.observe(Observation::Live(LivePosition::on("i4").then("i5")), 0);
        assert_eq!(
            follower.current_item().map(|i| i.title.as_str()),
            Some("Sermon — Part 3")
        );
        assert_eq!(
            follower.next_item().map(|i| i.title.as_str()),
            Some("Doxology")
        );
    }
}
