//! Scenes: named, savable snapshots of a setup's lighting, recalled with a fade.
//!
//! A [`Scene`] is the volunteer's noun — "pre-service", "worship", "sermon",
//! "blackout". It holds the levels of every DMX slot the setup's patch covers,
//! captured from the live buffer at the moment the user pressed *Save look*,
//! plus how long a recall should take to get there.
//!
//! **Sparse on purpose.** A scene names only the slots its setup patches, so
//! recalling one leaves every other slot exactly where it was — the same
//! overlay discipline as [`crate::buffer::LuxBuffer::set`]. A setup with no
//! fixtures captures the whole universe instead, because "no patch" means "the
//! plain universe" everywhere else in this app too (see [`crate::setup::Setup::compile`]).
//!
//! **Scenes are destinations; presets are momentary.** The preset engine in the
//! front end (`lib/preset-engine.ts`) exists to remember a frame and put it
//! back; a scene has no undo lane, because the way back from a scene is another
//! scene. Recall moves the buffer through the ordinary write path, so an engaged
//! preset drops its own marker through the reconcile it already runs — nothing
//! here duplicates or reaches into that machinery.
//!
//! This module owns the scene *domain* (types, capture, the operations over a
//! `Vec<Scene>`) and the recall driver. Persistence is the setup store's job,
//! exactly as it is for fixtures.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, Runtime};

use lux_engine::fade::Crossfade;

use crate::buffer::{Buffer, LuxBuffer, UNIVERSE_SIZE};
use crate::fixture::Fixture;
use crate::lock::LockPolicy;

/// How many scenes one setup may hold. A volunteer's Sunday needs five, not a
/// show file; the cap keeps a synced setup item small and bounds the UI.
pub const MAX_SCENES_PER_SETUP: usize = 64;

/// Longest recall a scene may ask for (one minute). Anything above is a typo.
pub const MAX_FADE_MS: u32 = 60_000;

/// Default crossfade for a freshly captured scene — slow enough to read as a
/// transition in a room, fast enough not to feel broken.
pub const DEFAULT_FADE_MS: u32 = 2_000;

/// One slot's level inside a scene. `ch` is the 1-based DMX slot, matching
/// `lux_wire::ctl::Frame::Channel` — the same names the wire already uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SceneLevel {
    pub ch: u16,
    pub val: u8,
}

/// A saved look: sparse levels plus the time a recall takes to reach them.
///
/// Order is the scene's position — scenes live in a `Vec` on the setup, so the
/// vector index *is* the position. There is deliberately no `position` field to
/// keep in step with it.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Scene {
    #[specta(type = String)]
    pub id: uuid::Uuid,
    pub name: String,
    /// Sparse levels, sorted by slot.
    pub levels: Vec<SceneLevel>,
    /// Crossfade duration on recall, in milliseconds. `0` snaps.
    pub fade_ms: u32,
}

// --- capture ----------------------------------------------------------------

/// Snapshot the slots `fixtures` covers, read from `buffer`.
///
/// An unpatched setup captures the whole universe: with no patch there is no
/// smaller honest answer, and the Universe view is a legitimate place to build
/// a look from.
pub fn capture_levels(buffer: &[u8], fixtures: &[Fixture]) -> Vec<SceneLevel> {
    let level = |ch: u16| -> Option<SceneLevel> {
        let index = usize::from(ch).checked_sub(1)?;
        buffer.get(index).map(|&val| SceneLevel { ch, val })
    };

    if fixtures.is_empty() {
        let last = u16::try_from(UNIVERSE_SIZE).unwrap_or(u16::MAX);
        return (1..=last).filter_map(level).collect();
    }

    let mut slots: Vec<u16> = fixtures.iter().flat_map(|f| f.address..=f.end()).collect();
    slots.sort_unstable();
    slots.dedup();
    slots.into_iter().filter_map(level).collect()
}

// --- operations (over a plain `Vec<Scene>`) ---------------------------------
//
// Storage-agnostic like `crate::fixture`: the scenes live inside whichever
// setup is active, and the caller persists the owning store.

fn clean_name(name: String) -> Result<String, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("scene name can't be empty".into());
    }
    Ok(name)
}

fn find(scenes: &mut [Scene], id: uuid::Uuid) -> Result<&mut Scene, String> {
    scenes
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("scene {id} not found"))
}

/// Append a captured look. A scene with no levels is refused: it would recall
/// nothing, which is indistinguishable from a broken button.
pub fn add(
    scenes: &mut Vec<Scene>,
    name: String,
    levels: Vec<SceneLevel>,
) -> Result<Scene, String> {
    if scenes.len() >= MAX_SCENES_PER_SETUP {
        return Err(format!(
            "this setup already holds {MAX_SCENES_PER_SETUP} scenes"
        ));
    }
    if levels.is_empty() {
        return Err("nothing to save — patch a fixture first".into());
    }
    let scene = Scene {
        id: uuid::Uuid::new_v4(),
        name: clean_name(name)?,
        levels,
        fade_ms: DEFAULT_FADE_MS,
    };
    scenes.push(scene.clone());
    Ok(scene)
}

/// Re-capture an existing scene's levels — the "I moved a fader, keep it" edit.
pub fn set_levels(
    scenes: &mut [Scene],
    id: uuid::Uuid,
    levels: Vec<SceneLevel>,
) -> Result<(), String> {
    if levels.is_empty() {
        return Err("nothing to save — patch a fixture first".into());
    }
    find(scenes, id)?.levels = levels;
    Ok(())
}

pub fn rename(scenes: &mut [Scene], id: uuid::Uuid, name: String) -> Result<(), String> {
    let name = clean_name(name)?;
    find(scenes, id)?.name = name;
    Ok(())
}

/// Set a scene's recall time, clamped to something a room can sit through.
pub fn set_fade(scenes: &mut [Scene], id: uuid::Uuid, fade_ms: u32) -> Result<(), String> {
    find(scenes, id)?.fade_ms = fade_ms.min(MAX_FADE_MS);
    Ok(())
}

/// Move a scene `delta` places in the list, saturating at either end (moving
/// the first scene left is a no-op, not an error — it is a button a user may
/// press twice).
pub fn move_by(scenes: &mut [Scene], id: uuid::Uuid, delta: i32) -> Result<(), String> {
    let from = scenes
        .iter()
        .position(|s| s.id == id)
        .ok_or_else(|| format!("scene {id} not found"))?;
    let last = scenes.len().saturating_sub(1);
    let step = usize::try_from(delta.unsigned_abs()).unwrap_or(usize::MAX);
    let to = if delta < 0 {
        from.saturating_sub(step)
    } else {
        from.saturating_add(step).min(last)
    };
    // Rotating the span between the two positions moves the scene and shuffles
    // everything it passed by one place — no clone, no hole to fill.
    if to > from {
        scenes[from..=to].rotate_left(1);
    } else if to < from {
        scenes[to..=from].rotate_right(1);
    }
    Ok(())
}

pub fn remove(scenes: &mut Vec<Scene>, id: uuid::Uuid) -> Result<(), String> {
    let before = scenes.len();
    scenes.retain(|s| s.id != id);
    if scenes.len() == before {
        return Err(format!("scene {id} not found"));
    }
    Ok(())
}

// --- recall (the crossfade driver) ------------------------------------------

/// How often the fade writes. 40 Hz sits under DMX512's ~44 Hz frame ceiling
/// and matches what a fader drag already produces, so nothing downstream —
/// render, debounced persist, coalesced state echo — sees a new load shape.
const TICK: Duration = Duration::from_millis(25);

/// Which recall owns the rig. Every [`recall`] bumps the generation; an
/// in-flight ticker that no longer holds the current one stops at its next
/// tick. That is the whole cancellation story: **the last recall wins**, with
/// no channels, no join handles, and no torn frames from two fades writing the
/// same slot.
#[derive(Debug, Default)]
pub struct LuxFade {
    generation: AtomicU64,
}

/// Start recalling `scene`: crossfade every slot it owns from wherever the rig
/// is *now* to the saved levels, over the scene's fade time.
///
/// Returns as soon as the fade is running (immediately, for a snap). The fade
/// writes through [`LuxBuffer::apply_levels`], so persistence, the remote state
/// echo, shared desks and DMX output all behave exactly as they do for a fader
/// — there is no second output path to keep correct.
pub fn recall<R: Runtime>(app: &AppHandle<R>, scene: &Scene) -> Result<(), String> {
    let targets: Vec<(u16, u8)> = scene.levels.iter().map(|l| (l.ch, l.val)).collect();
    let from: Buffer = app.state::<LuxBuffer>().buffer.lock_or_recover().clone();
    let fade = Crossfade::new(&from, &targets, scene.fade_ms);
    let generation = app
        .state::<LuxFade>()
        .generation
        .fetch_add(1, Ordering::SeqCst)
        .saturating_add(1);

    // A snap has nothing to schedule: apply it on the calling thread so a
    // zero-fade scene is as immediate as pressing Blackout.
    if fade.is_done(0) {
        let mut buffer = app.state::<LuxBuffer>().inner().clone();
        buffer.apply_levels(&fade.at(0), app.clone())?;
        return Ok(());
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let started = Instant::now();
        loop {
            tokio::time::sleep(TICK).await;
            if app.state::<LuxFade>().generation.load(Ordering::SeqCst) != generation {
                return; // a newer recall (or a blackout) owns the rig now
            }
            let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            let mut buffer = app.state::<LuxBuffer>().inner().clone();
            if let Err(e) = buffer.apply_levels(&fade.at(elapsed), app.clone()) {
                // A missing output is not a reason to abandon the fade: the
                // buffer is still the truth, and the user may plug in mid-fade.
                log::trace!("scene fade write failed: {e}");
            }
            if fade.is_done(elapsed) {
                return;
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::colors::LuxLabelColor;
    use crate::fixture::ChannelDef;

    fn fixture(name: &str, address: u16, count: usize) -> Fixture {
        Fixture {
            id: uuid::Uuid::new_v4(),
            name: name.to_owned(),
            address,
            channels: (0..count)
                .map(|_| ChannelDef {
                    role: LuxLabelColor::Generic,
                    label: "ch".into(),
                })
                .collect(),
        }
    }

    fn buffer() -> Vec<u8> {
        (0..UNIVERSE_SIZE)
            .map(|i| u8::try_from(i % 256).unwrap_or(0))
            .collect()
    }

    fn levels(pairs: &[(u16, u8)]) -> Vec<SceneLevel> {
        pairs
            .iter()
            .map(|&(ch, val)| SceneLevel { ch, val })
            .collect()
    }

    #[test]
    fn capture_covers_the_patch_in_slot_order() {
        // Patched out of order, and overlapping nothing: capture is a universe
        // read, so it comes back sorted regardless of patch order.
        let fixtures = vec![fixture("Back", 10, 3), fixture("Front", 1, 2)];
        let captured = capture_levels(&buffer(), &fixtures);
        assert_eq!(
            captured.iter().map(|l| l.ch).collect::<Vec<_>>(),
            vec![1, 2, 10, 11, 12]
        );
        assert_eq!(captured[0].val, 0); // slot 1 → buffer[0]
        assert_eq!(captured[2].val, 9); // slot 10 → buffer[9]
    }

    #[test]
    fn capture_on_an_unpatched_setup_takes_the_whole_universe() {
        let captured = capture_levels(&buffer(), &[]);
        assert_eq!(captured.len(), UNIVERSE_SIZE);
        assert_eq!(captured[0].ch, 1);
        assert_eq!(captured[UNIVERSE_SIZE - 1].ch, 512);
    }

    #[test]
    fn capture_ignores_slots_past_the_universe() {
        // A fixture at the very top of the universe: nothing beyond 512 appears.
        let captured = capture_levels(&buffer(), &[fixture("Edge", 511, 2)]);
        assert_eq!(
            captured.iter().map(|l| l.ch).collect::<Vec<_>>(),
            vec![511, 512]
        );
    }

    #[test]
    fn add_names_validates_and_caps() {
        let mut scenes = Vec::new();
        assert!(add(&mut scenes, "  ".into(), levels(&[(1, 5)])).is_err());
        assert!(add(&mut scenes, "Empty".into(), vec![]).is_err());

        let scene = add(&mut scenes, "  Worship  ".into(), levels(&[(1, 5)])).unwrap();
        assert_eq!(scene.name, "Worship"); // trimmed
        assert_eq!(scene.fade_ms, DEFAULT_FADE_MS);
        assert_eq!(scenes.len(), 1);

        while scenes.len() < MAX_SCENES_PER_SETUP {
            add(&mut scenes, "filler".into(), levels(&[(1, 5)])).unwrap();
        }
        assert!(add(&mut scenes, "one too many".into(), levels(&[(1, 5)])).is_err());
    }

    #[test]
    fn rename_set_levels_and_fade() {
        let mut scenes = Vec::new();
        let id = add(&mut scenes, "Worship".into(), levels(&[(1, 5)]))
            .unwrap()
            .id;

        rename(&mut scenes, id, " Sermon ".into()).unwrap();
        assert_eq!(scenes[0].name, "Sermon");
        assert!(rename(&mut scenes, id, "".into()).is_err());
        assert!(rename(&mut scenes, uuid::Uuid::new_v4(), "ghost".into()).is_err());

        set_levels(&mut scenes, id, levels(&[(1, 200), (2, 10)])).unwrap();
        assert_eq!(scenes[0].levels.len(), 2);
        assert!(set_levels(&mut scenes, id, vec![]).is_err());

        set_fade(&mut scenes, id, 500).unwrap();
        assert_eq!(scenes[0].fade_ms, 500);
        // An absurd fade clamps rather than erroring — it came from a picker.
        set_fade(&mut scenes, id, u32::MAX).unwrap();
        assert_eq!(scenes[0].fade_ms, MAX_FADE_MS);
    }

    #[test]
    fn move_by_reorders_and_saturates() {
        let mut scenes = Vec::new();
        for name in ["a", "b", "c"] {
            add(&mut scenes, name.into(), levels(&[(1, 5)])).unwrap();
        }
        let names = |s: &[Scene]| s.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
        let a = scenes[0].id;
        let c = scenes[2].id;

        move_by(&mut scenes, a, 1).unwrap();
        assert_eq!(names(&scenes), vec!["b", "a", "c"]);
        move_by(&mut scenes, c, -2).unwrap();
        assert_eq!(names(&scenes), vec!["c", "b", "a"]);

        // Pressing "move left" on the first scene is a no-op, not an error.
        move_by(&mut scenes, c, -1).unwrap();
        assert_eq!(names(&scenes), vec!["c", "b", "a"]);
        move_by(&mut scenes, a, 9).unwrap();
        assert_eq!(names(&scenes), vec!["c", "b", "a"]);
        assert!(move_by(&mut scenes, uuid::Uuid::new_v4(), 1).is_err());
    }

    #[test]
    fn remove_deletes_and_reports_missing() {
        let mut scenes = Vec::new();
        let id = add(&mut scenes, "Worship".into(), levels(&[(1, 5)]))
            .unwrap()
            .id;
        remove(&mut scenes, id).unwrap();
        assert!(scenes.is_empty());
        assert!(remove(&mut scenes, id).is_err());
    }

    /// Capture → crossfade → apply, the seam the two halves of this brick meet
    /// at, exercised without Tauri: the sparse levels a capture produces must
    /// be exactly what a recall lands on, and nothing else may move.
    #[test]
    fn a_captured_look_recalls_to_itself_and_leaves_the_rest_alone() {
        let fixtures = vec![fixture("Front", 1, 3)];
        let mut saved = vec![0u8; UNIVERSE_SIZE];
        saved[0] = 255;
        saved[1] = 128;
        saved[2] = 10;
        saved[400] = 77; // an unpatched slot a raw fader owns
        let captured = capture_levels(&saved, &fixtures);

        // Someone has since moved everything, including the raw fader.
        let mut live = vec![9u8; UNIVERSE_SIZE];
        let targets: Vec<(u16, u8)> = captured.iter().map(|l| (l.ch, l.val)).collect();
        let fade = Crossfade::new(&live, &targets, 1000);

        // Mid-fade nothing has arrived yet, and the fade owns three slots only.
        assert_eq!(fade.at(500).len(), 3);

        for (ch, val) in fade.at(1000) {
            live[usize::from(ch) - 1] = val;
        }
        assert_eq!(&live[..3], &saved[..3]); // the look is back, exactly
        assert_eq!(live[400], 9); // the raw fader was never in the scene
    }

    #[test]
    fn scenes_round_trip_through_json() {
        let mut scenes = Vec::new();
        add(&mut scenes, "Worship".into(), levels(&[(1, 5), (2, 255)])).unwrap();
        let json = serde_json::to_string(&scenes).unwrap();
        assert!(json.contains(r#""levels":[{"ch":1,"val":5},{"ch":2,"val":255}]"#));
        let back: Vec<Scene> = serde_json::from_str(&json).unwrap();
        assert_eq!(back[0].levels, scenes[0].levels);
        assert_eq!(back[0].fade_ms, DEFAULT_FADE_MS);
    }
}
