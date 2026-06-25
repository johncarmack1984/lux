//! User-patched fixtures: named, typed windows onto ranges of the DMX universe.
//!
//! A [`Fixture`] is `channels.len()` consecutive DMX slots starting at a 1-based
//! `address`; each [`ChannelDef`] gives that slot a role (which drives the UI
//! control + colour) and a label. Fixtures are additive: their controls write the
//! shared `LuxBuffer` through the normal overlay `set` path, so the raw universe
//! desk and color picker are unaffected.
//!
//! This module owns the fixture *domain* — the types, the preset library, and the
//! placement validation + add/update/remove operations over a `Vec<Fixture>`. It
//! is deliberately storage-agnostic: a patch is just a `Vec<Fixture>` that lives
//! inside whichever [`crate::setup::Setup`] is active, and persistence is the
//! setup store's job.

use serde::{Deserialize, Serialize};
use specta::Type;

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

/// The default single RGBAW fixture at slot 1 — the original PoC patch, now the
/// seed for a brand-new "Home" setup on first run.
pub fn default_fixtures() -> Vec<Fixture> {
    let channels = presets()
        .into_iter()
        .find(|p| p.key == "rgbaw")
        .map(|p| p.channels)
        .unwrap_or_default();
    vec![Fixture {
        id: uuid::Uuid::new_v4(),
        name: "RGBAW".into(),
        address: 1,
        channels,
    }]
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

// --- patch operations (over a plain `Vec<Fixture>`) -------------------------
//
// The patch lives inside the active setup; these helpers mutate that vec in
// place after validating placement, and the caller persists the owning store.

/// Add a fixture to `fixtures`, validating placement against the existing ones.
pub fn add(
    fixtures: &mut Vec<Fixture>,
    name: String,
    address: u16,
    channels: Vec<ChannelDef>,
) -> Result<Fixture, String> {
    validate_placement(address, channels.len(), fixtures)?;
    let fixture = Fixture {
        id: uuid::Uuid::new_v4(),
        name,
        address,
        channels,
    };
    fixtures.push(fixture.clone());
    Ok(fixture)
}

/// Move/relabel the fixture `id` in place. Validation runs against the *other*
/// fixtures, so a fixture may overlap its own current slots while moving.
pub fn update(
    fixtures: &mut [Fixture],
    id: uuid::Uuid,
    name: String,
    address: u16,
    channels: Vec<ChannelDef>,
) -> Result<Fixture, String> {
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

/// Remove the fixture `id`, reporting an error if it wasn't present.
pub fn remove(fixtures: &mut Vec<Fixture>, id: uuid::Uuid) -> Result<(), String> {
    let before = fixtures.len();
    fixtures.retain(|f| f.id != id);
    if fixtures.len() == before {
        return Err(format!("fixture {id} not found"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn default_fixtures_is_one_rgbaw_at_slot_one() {
        let f = default_fixtures();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].address, 1);
        assert_eq!(f[0].channels.len(), 6);
    }

    #[test]
    fn add_then_roundtrips() {
        let mut fixtures = vec![];
        let f = add(&mut fixtures, "Left".into(), 1, six()).unwrap();
        assert_eq!(fixtures.len(), 1);
        assert_eq!(f.address, 1);
    }

    #[test]
    fn rejects_overlap_but_allows_adjacent() {
        let mut fixtures = vec![];
        add(&mut fixtures, "Left".into(), 1, six()).unwrap(); // slots 1..=6
        let err = add(&mut fixtures, "Right".into(), 6, six()).unwrap_err(); // 6..=11 overlaps at 6
        assert!(err.contains("overlaps"), "{err}");
        add(&mut fixtures, "Right".into(), 7, six()).unwrap(); // 7..=12 is adjacent, fine
        assert_eq!(fixtures.len(), 2);
    }

    #[test]
    fn rejects_out_of_range() {
        let mut fixtures = vec![];
        assert!(add(&mut fixtures, "x".into(), 511, six()).is_err()); // 511..=516 > 512
        assert!(add(&mut fixtures, "x".into(), 0, six()).is_err());
        assert!(add(&mut fixtures, "x".into(), 1, vec![]).is_err()); // no channels
    }

    #[test]
    fn update_can_move_a_fixture_onto_its_own_slots() {
        let mut fixtures = vec![];
        let f = add(&mut fixtures, "Left".into(), 1, six()).unwrap();
        update(&mut fixtures, f.id, "Left".into(), 2, six()).unwrap(); // overlaps old self only — ok
        assert_eq!(fixtures[0].address, 2);
        assert!(update(&mut fixtures, uuid::Uuid::new_v4(), "ghost".into(), 1, six()).is_err());
    }

    #[test]
    fn remove_deletes_and_reports_missing() {
        let mut fixtures = vec![];
        let f = add(&mut fixtures, "x".into(), 1, six()).unwrap();
        remove(&mut fixtures, f.id).unwrap();
        assert!(fixtures.is_empty());
        assert!(remove(&mut fixtures, f.id).is_err());
    }
}
