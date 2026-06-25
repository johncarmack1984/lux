//! User-patched fixtures: named, typed windows onto ranges of the DMX universe.
//!
//! A [`Fixture`] is `channels.len()` consecutive DMX slots starting at a 1-based
//! `address`; each [`ChannelDef`] gives that slot a role (which drives the UI
//! control + colour) and a label. The set of fixtures is the [`LuxPatch`], held
//! in Tauri state and persisted to `app_config_dir()/patch.json` — the same
//! lightweight pattern the tray uses for the selected DMX device. Fixtures are
//! additive: their controls write the shared `LuxBuffer` through the normal
//! overlay `set` path, so the raw universe desk and color picker are unaffected.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{Manager, Runtime};

use crate::buffer::UNIVERSE_SIZE;
use crate::colors::LuxLabelColor;

/// One channel within a fixture.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelDef {
    /// Semantic role — drives the colour swatch / control affordance.
    pub role: LuxLabelColor,
    pub label: String,
}

/// A patched fixture: `channels.len()` consecutive slots from `address` (1-based).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Fixture {
    #[specta(type = String)]
    pub id: uuid::Uuid,
    pub name: String,
    pub address: u16,
    pub channels: Vec<ChannelDef>,
}

impl Fixture {
    /// Last DMX slot this fixture occupies (1-based, inclusive).
    fn end(&self) -> u16 {
        self.address + self.channels.len() as u16 - 1
    }
}

/// A built-in starting point for the "new fixture" flow.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FixturePreset {
    pub key: String,
    pub name: String,
    pub channels: Vec<ChannelDef>,
}

fn ch(role: LuxLabelColor, label: &str) -> ChannelDef {
    ChannelDef {
        role,
        label: label.to_owned(),
    }
}

/// The built-in preset library. Custom fixtures start from one of these (or
/// blank) and edit channels freely, so this is just a convenience seed.
pub fn presets() -> Vec<FixturePreset> {
    use LuxLabelColor::*;
    let preset = |key: &str, name: &str, channels: Vec<ChannelDef>| FixturePreset {
        key: key.to_owned(),
        name: name.to_owned(),
        channels,
    };
    vec![
        preset(
            "rgbaw",
            "RGBAW",
            vec![
                ch(Red, "Red"),
                ch(Green, "Green"),
                ch(Blue, "Blue"),
                ch(Amber, "Amber"),
                ch(White, "White"),
                ch(Brightness, "Dimmer"),
            ],
        ),
        preset(
            "rgb",
            "RGB",
            vec![ch(Red, "Red"), ch(Green, "Green"), ch(Blue, "Blue")],
        ),
        preset(
            "rgbw",
            "RGBW",
            vec![
                ch(Red, "Red"),
                ch(Green, "Green"),
                ch(Blue, "Blue"),
                ch(White, "White"),
            ],
        ),
        preset(
            "rgb-dimmer",
            "RGB + Dimmer",
            vec![
                ch(Red, "Red"),
                ch(Green, "Green"),
                ch(Blue, "Blue"),
                ch(Brightness, "Dimmer"),
            ],
        ),
        preset("dimmer", "Dimmer", vec![ch(Brightness, "Dimmer")]),
    ]
}

/// Validate a fixture placement against the rest of the patch. Pure (no lock /
/// `AppHandle`) so it is unit-testable.
fn validate_placement(address: u16, len: usize, others: &[Fixture]) -> Result<(), String> {
    if len == 0 {
        return Err("a fixture needs at least one channel".into());
    }
    if address < 1 {
        return Err("address must be 1 or greater".into());
    }
    let end = address as usize + len - 1;
    if end > UNIVERSE_SIZE {
        return Err(format!(
            "fixture spans slots {address}..={end}, past the {UNIVERSE_SIZE}-channel universe"
        ));
    }
    let end = end as u16;
    for other in others {
        // Inclusive ranges [address, end] and [other.address, other.end()] overlap.
        if address <= other.end() && other.address <= end {
            return Err(format!(
                "overlaps \"{}\" (slots {}..={})",
                other.name,
                other.address,
                other.end()
            ));
        }
    }
    Ok(())
}

/// Tauri-managed set of patched fixtures.
#[derive(Debug)]
pub struct LuxPatch {
    pub fixtures: Arc<Mutex<Vec<Fixture>>>,
}

impl Default for LuxPatch {
    fn default() -> Self {
        // Default patch reproduces the original single RGBAW fixture at slot 1.
        let channels = presets()
            .into_iter()
            .find(|p| p.key == "rgbaw")
            .map(|p| p.channels)
            .unwrap_or_default();
        let fixture = Fixture {
            id: uuid::Uuid::new_v4(),
            name: "RGBAW".into(),
            address: 1,
            channels,
        };
        LuxPatch {
            fixtures: Arc::new(Mutex::new(vec![fixture])),
        }
    }
}

impl LuxPatch {
    pub fn list(&self) -> Vec<Fixture> {
        self.fixtures.lock().unwrap().clone()
    }

    pub fn add(
        &self,
        name: String,
        address: u16,
        channels: Vec<ChannelDef>,
    ) -> Result<Fixture, String> {
        let mut fixtures = self.fixtures.lock().unwrap();
        validate_placement(address, channels.len(), &fixtures)?;
        let fixture = Fixture {
            id: uuid::Uuid::new_v4(),
            name,
            address,
            channels,
        };
        fixtures.push(fixture.clone());
        Ok(fixture)
    }

    pub fn update(
        &self,
        id: uuid::Uuid,
        name: String,
        address: u16,
        channels: Vec<ChannelDef>,
    ) -> Result<Fixture, String> {
        let mut fixtures = self.fixtures.lock().unwrap();
        // Validate against the *other* fixtures so a fixture can move within or
        // across its own current slots.
        let others: Vec<Fixture> = fixtures.iter().filter(|f| f.id != id).cloned().collect();
        if others.len() == fixtures.len() {
            return Err(format!("fixture {id} not found"));
        }
        validate_placement(address, channels.len(), &others)?;
        let fixture = Fixture {
            id,
            name,
            address,
            channels,
        };
        if let Some(slot) = fixtures.iter_mut().find(|f| f.id == id) {
            *slot = fixture.clone();
        }
        Ok(fixture)
    }

    pub fn remove(&self, id: uuid::Uuid) -> Result<(), String> {
        let mut fixtures = self.fixtures.lock().unwrap();
        let before = fixtures.len();
        fixtures.retain(|f| f.id != id);
        if fixtures.len() == before {
            return Err(format!("fixture {id} not found"));
        }
        Ok(())
    }
}

// --- persistence (app_config_dir/patch.json) --------------------------------

fn patch_file<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("patch.json"))
}

/// Load the patch from disk, or the default patch when absent/unreadable. An
/// empty file (`[]`) is honoured — that's a user who deleted every fixture.
pub fn load<R: Runtime>(app: &tauri::AppHandle<R>) -> LuxPatch {
    let Some(path) = patch_file(app) else {
        return LuxPatch::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(json) => match serde_json::from_str::<Vec<Fixture>>(&json) {
            Ok(fixtures) => LuxPatch {
                fixtures: Arc::new(Mutex::new(fixtures)),
            },
            Err(e) => {
                log::warn!("patch.json unreadable ({e}); using default patch");
                LuxPatch::default()
            }
        },
        // Absent file → first run → default patch.
        Err(_) => LuxPatch::default(),
    }
}

/// Persist the current patch to disk. Best-effort (logs on failure).
pub fn save<R: Runtime>(app: &tauri::AppHandle<R>, patch: &LuxPatch) {
    let Some(path) = patch_file(app) else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match serde_json::to_string_pretty(&patch.list()) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                log::warn!("could not persist patch: {e}");
            }
        }
        Err(e) => log::warn!("could not serialize patch: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> LuxPatch {
        LuxPatch {
            fixtures: Arc::new(Mutex::new(vec![])),
        }
    }

    fn six() -> Vec<ChannelDef> {
        presets()
            .into_iter()
            .find(|p| p.key == "rgbaw")
            .unwrap()
            .channels
    }

    #[test]
    fn presets_have_expected_shapes() {
        let p = presets();
        assert_eq!(p.iter().find(|p| p.key == "rgbaw").unwrap().channels.len(), 6);
        assert!(p.iter().any(|p| p.key == "dimmer"));
    }

    #[test]
    fn add_then_list_roundtrips() {
        let patch = empty();
        let f = patch.add("Left".into(), 1, six()).unwrap();
        assert_eq!(patch.list().len(), 1);
        assert_eq!(f.address, 1);
    }

    #[test]
    fn rejects_overlap_but_allows_adjacent() {
        let patch = empty();
        patch.add("Left".into(), 1, six()).unwrap(); // slots 1..=6
        let err = patch.add("Right".into(), 6, six()).unwrap_err(); // 6..=11 overlaps at slot 6
        assert!(err.contains("overlaps"), "{err}");
        patch.add("Right".into(), 7, six()).unwrap(); // 7..=12 is adjacent, fine
        assert_eq!(patch.list().len(), 2);
    }

    #[test]
    fn rejects_out_of_range() {
        let patch = empty();
        assert!(patch.add("x".into(), 511, six()).is_err()); // 511..=516 > 512
        assert!(patch.add("x".into(), 0, six()).is_err());
        assert!(patch.add("x".into(), 1, vec![]).is_err()); // no channels
    }

    #[test]
    fn update_can_move_a_fixture_onto_its_own_slots() {
        let patch = empty();
        let f = patch.add("Left".into(), 1, six()).unwrap();
        patch.update(f.id, "Left".into(), 2, six()).unwrap(); // overlaps old self only — ok
        assert_eq!(patch.list()[0].address, 2);
        assert!(patch.update(uuid::Uuid::new_v4(), "ghost".into(), 1, six()).is_err());
    }

    #[test]
    fn remove_deletes_and_reports_missing() {
        let patch = empty();
        let f = patch.add("x".into(), 1, six()).unwrap();
        patch.remove(f.id).unwrap();
        assert!(patch.list().is_empty());
        assert!(patch.remove(f.id).is_err());
    }
}
