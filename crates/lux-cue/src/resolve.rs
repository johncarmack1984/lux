//! Resolving a plan's items to scenes through a cue map.
//!
//! The rule order is the product, so it is stated once, here, and nowhere
//! else. For each item, in this order:
//!
//! 1. **Pin** — a rule naming the library song this item plays.
//! 2. **Title** — a rule whose pattern matches the item's title.
//! 3. **Item type** — a rule for `song`, `header`, `media`, … .
//! 4. **Fallback** — the map's `fallbackSceneId`, if it has one.
//! 5. Nothing. An unmapped item changes no lights, which is not a failure:
//!    it is the rig staying where the last cue left it.
//!
//! Tiers beat list order — a pin buried at the bottom of the list still wins
//! over a type rule at the top, because "this song gets this look" is the more
//! specific statement and an operator should not have to know the ordering
//! rules to get what they meant. *Within* a tier the first matching rule wins,
//! so the list order the operator sees on the surface is the tiebreak they
//! control.
//!
//! Matching is case- and whitespace-insensitive on both sides: real plans are
//! typed by volunteers, in a hurry, on a phone.

use lux_wire::plan::{CueMap, CueRule, TitleMode};

use crate::PlanItem;

/// Which tier of the cue map chose a cue's scene. Carried so a surface can
/// tell the operator *why* an item lit the way it did — the difference between
/// "your pin" and "the fallback" is the difference between a map that works
/// and a map that only looks like it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CueSource {
    Pin,
    Title,
    ItemType,
    Fallback,
    /// No rule matched and the map has no fallback.
    Unmapped,
}

/// One plan item and the scene it calls for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cue {
    pub item: PlanItem,
    /// `None` when nothing matched — hold, never blackout.
    pub scene_id: Option<String>,
    pub source: CueSource,
}

/// A whole plan resolved: one cue per item, in plan order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CueSheet {
    cues: Vec<Cue>,
}

impl CueSheet {
    /// Resolve `items` against `map`. Items are sorted by their plan sequence
    /// first: the follow engine walks this list to answer "what's next", and a
    /// list that trusted the transport's ordering would be a bug that only
    /// appears on the one Sunday the API pages differently.
    pub fn resolve(map: &CueMap, items: &[PlanItem]) -> Self {
        let mut items = items.to_vec();
        items.sort_by(|a, b| a.sequence.cmp(&b.sequence).then_with(|| a.id.cmp(&b.id)));
        let cues = items
            .into_iter()
            .map(|item| {
                let (scene_id, source) = match_item(map, &item);
                Cue {
                    item,
                    scene_id,
                    source,
                }
            })
            .collect();
        Self { cues }
    }

    /// An empty sheet — a follower built before the plan has been pulled, and
    /// what a plan with no items resolves to.
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn cues(&self) -> &[Cue] {
        &self.cues
    }

    pub fn len(&self) -> usize {
        self.cues.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cues.is_empty()
    }

    pub fn cue_for(&self, item_id: &str) -> Option<&Cue> {
        self.cues.iter().find(|c| c.item.id == item_id)
    }

    /// The scene an item calls for, if the item is in this plan and mapped.
    pub fn scene_for(&self, item_id: &str) -> Option<&str> {
        self.cue_for(item_id)?.scene_id.as_deref()
    }

    /// Whether the plan holds this item at all — the question that separates
    /// "unmapped" from "the plan changed under us".
    pub fn contains(&self, item_id: &str) -> bool {
        self.cue_for(item_id).is_some()
    }

    pub fn index_of(&self, item_id: &str) -> Option<usize> {
        self.cues.iter().position(|c| c.item.id == item_id)
    }

    pub fn get(&self, index: usize) -> Option<&Cue> {
        self.cues.get(index)
    }
}

/// The rule ladder, applied to one item.
fn match_item(map: &CueMap, item: &PlanItem) -> (Option<String>, CueSource) {
    let pin = map.rules.iter().find_map(|rule| match rule {
        CueRule::Pin { song_id, scene_id } => {
            (item.song_id.as_deref() == Some(song_id.as_str())).then(|| scene_id.clone())
        }
        _ => None,
    });
    if let Some(scene_id) = pin {
        return (Some(scene_id), CueSource::Pin);
    }

    let title = map.rules.iter().find_map(|rule| match rule {
        CueRule::Title {
            pattern,
            mode,
            scene_id,
        } => title_matches(&item.title, pattern, *mode).then(|| scene_id.clone()),
        _ => None,
    });
    if let Some(scene_id) = title {
        return (Some(scene_id), CueSource::Title);
    }

    let by_type = map.rules.iter().find_map(|rule| match rule {
        CueRule::ItemType {
            item_type,
            scene_id,
        } => normalize(item_type)
            .eq(&normalize(&item.item_type))
            .then(|| scene_id.clone()),
        _ => None,
    });
    if let Some(scene_id) = by_type {
        return (Some(scene_id), CueSource::ItemType);
    }

    match &map.fallback_scene_id {
        Some(scene_id) => (Some(scene_id.clone()), CueSource::Fallback),
        None => (None, CueSource::Unmapped),
    }
}

fn title_matches(title: &str, pattern: &str, mode: TitleMode) -> bool {
    let pattern = normalize(pattern);
    // An empty pattern is an authoring slip (a half-typed rule, a cleared
    // field). Matching everything would be the loudest possible reading of it,
    // so it matches nothing instead.
    if pattern.is_empty() {
        return false;
    }
    let title = normalize(title);
    match mode {
        TitleMode::Contains => title.contains(&pattern),
        TitleMode::Exact => title == pattern,
    }
}

/// Lowercased, with every run of whitespace collapsed to one space and the
/// ends trimmed — so "  SERMON —  part 3 " and "Sermon — Part 3" are the same
/// title, which is what a volunteer typing at 8am means by it.
fn normalize(s: &str) -> String {
    s.split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use lux_wire::plan::CueMap;

    fn sunday() -> Vec<PlanItem> {
        vec![
            PlanItem::new("i1", 1, "Pre-Service", "header"),
            PlanItem::new("i2", 2, "Great Are You Lord", "song").with_song("1001"),
            PlanItem::new("i3", 3, "King of Kings", "song").with_song("1002"),
            PlanItem::new("i4", 4, "Welcome & Announcements", "header"),
            PlanItem::new("i5", 5, "Bumper Video", "media"),
            PlanItem::new("i6", 6, "Sermon — Part 3", "header"),
            PlanItem::new("i7", 7, "Doxology", "song").with_song("1003"),
            PlanItem::new("i8", 8, "Dismissal", "item"),
        ]
    }

    fn map() -> CueMap {
        CueMap::new(
            "st-1".into(),
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
                // Listed last on purpose: a pin still outranks the type rule
                // above it.
                CueRule::Pin {
                    song_id: "1003".into(),
                    scene_id: "doxology".into(),
                },
            ],
        )
        .with_fallback("house".into())
    }

    fn scenes(sheet: &CueSheet) -> Vec<Option<&str>> {
        sheet.cues().iter().map(|c| c.scene_id.as_deref()).collect()
    }

    #[test]
    fn a_whole_sunday_resolves_in_priority_order() {
        let sheet = CueSheet::resolve(&map(), &sunday());
        assert_eq!(
            scenes(&sheet),
            vec![
                Some("house"),   // header, nothing matched -> fallback
                Some("worship"), // song by type
                Some("worship"),
                Some("announce"), // title beats the (absent) header type rule
                Some("video"),
                Some("sermon"),
                Some("doxology"), // pin beats the song type rule
                Some("house"),
            ]
        );
        assert_eq!(
            sheet.cues().iter().map(|c| c.source).collect::<Vec<_>>(),
            vec![
                CueSource::Fallback,
                CueSource::ItemType,
                CueSource::ItemType,
                CueSource::Title,
                CueSource::ItemType,
                CueSource::Title,
                CueSource::Pin,
                CueSource::Fallback,
            ]
        );
    }

    #[test]
    fn next_weeks_plan_inherits_the_map() {
        // Same songs and segments, brand-new item ids and a shuffled order:
        // the map is untouched and every item still lands on its scene. This
        // is the product in one test.
        let next_week = vec![
            PlanItem::new("j9", 1, "Welcome & Announcements", "header"),
            PlanItem::new("j7", 2, "Doxology", "song").with_song("1003"),
            PlanItem::new("j3", 3, "A Song We Have Never Played", "song").with_song("2044"),
            PlanItem::new("j1", 4, "Sermon — Part 4", "header"),
        ];
        let sheet = CueSheet::resolve(&map(), &next_week);
        assert_eq!(
            scenes(&sheet),
            vec![
                Some("announce"),
                Some("doxology"),
                Some("worship"),
                Some("sermon")
            ]
        );
    }

    #[test]
    fn items_are_sorted_by_plan_sequence() {
        let jumbled = vec![
            PlanItem::new("b", 2, "second", "item"),
            PlanItem::new("c", 3, "third", "item"),
            PlanItem::new("a", 1, "first", "item"),
        ];
        let sheet = CueSheet::resolve(&CueMap::new("st-1".into(), vec![]), &jumbled);
        assert_eq!(
            sheet
                .cues()
                .iter()
                .map(|c| c.item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert_eq!(sheet.index_of("c"), Some(2));
    }

    #[test]
    fn an_unmatched_item_with_no_fallback_holds() {
        let map = CueMap::new(
            "st-1".into(),
            vec![CueRule::ItemType {
                item_type: "song".into(),
                scene_id: "worship".into(),
            }],
        );
        let sheet = CueSheet::resolve(&map, &[PlanItem::new("i1", 1, "Offering", "item")]);
        assert_eq!(sheet.scene_for("i1"), None);
        assert_eq!(sheet.cues()[0].source, CueSource::Unmapped);
        assert!(sheet.contains("i1"));
        assert!(!sheet.contains("nope"));
    }

    #[test]
    fn matching_ignores_case_and_stray_whitespace() {
        let map = CueMap::new(
            "st-1".into(),
            vec![
                CueRule::Title {
                    pattern: "  SERMON  ".into(),
                    mode: TitleMode::Contains,
                    scene_id: "sermon".into(),
                },
                CueRule::ItemType {
                    item_type: "SONG".into(),
                    scene_id: "worship".into(),
                },
            ],
        );
        let items = vec![
            PlanItem::new("i1", 1, "Sermon  —   part 3", "header"),
            PlanItem::new("i2", 2, "Anything", "song"),
        ];
        let sheet = CueSheet::resolve(&map, &items);
        assert_eq!(scenes(&sheet), vec![Some("sermon"), Some("worship")]);
    }

    #[test]
    fn exact_mode_needs_the_whole_title() {
        let map = CueMap::new(
            "st-1".into(),
            vec![CueRule::Title {
                pattern: "Sermon".into(),
                mode: TitleMode::Exact,
                scene_id: "sermon".into(),
            }],
        );
        let items = vec![
            PlanItem::new("i1", 1, "Sermon", "header"),
            PlanItem::new("i2", 2, "Sermon — Part 3", "header"),
        ];
        let sheet = CueSheet::resolve(&map, &items);
        assert_eq!(scenes(&sheet), vec![Some("sermon"), None]);
    }

    #[test]
    fn an_empty_title_pattern_matches_nothing() {
        let map = CueMap::new(
            "st-1".into(),
            vec![CueRule::Title {
                pattern: "   ".into(),
                mode: TitleMode::Contains,
                scene_id: "oops".into(),
            }],
        );
        let sheet = CueSheet::resolve(&map, &[PlanItem::new("i1", 1, "Anything", "item")]);
        assert_eq!(sheet.scene_for("i1"), None);
    }

    #[test]
    fn within_a_tier_the_first_listed_rule_wins() {
        let map = CueMap::new(
            "st-1".into(),
            vec![
                CueRule::Title {
                    pattern: "part".into(),
                    mode: TitleMode::Contains,
                    scene_id: "first".into(),
                },
                CueRule::Title {
                    pattern: "sermon".into(),
                    mode: TitleMode::Contains,
                    scene_id: "second".into(),
                },
            ],
        );
        let sheet = CueSheet::resolve(&map, &[PlanItem::new("i1", 1, "Sermon — Part 3", "header")]);
        assert_eq!(sheet.scene_for("i1"), Some("first"));
    }

    #[test]
    fn a_pin_only_matches_the_song_it_names() {
        let map = CueMap::new(
            "st-1".into(),
            vec![CueRule::Pin {
                song_id: "1003".into(),
                scene_id: "doxology".into(),
            }],
        );
        let items = vec![
            // Same title, different song in the library: the pin follows the
            // song, not the words a volunteer typed.
            PlanItem::new("i1", 1, "Doxology", "song").with_song("9999"),
            PlanItem::new("i2", 2, "Doxology", "song").with_song("1003"),
            PlanItem::new("i3", 3, "Doxology", "header"),
        ];
        let sheet = CueSheet::resolve(&map, &items);
        assert_eq!(scenes(&sheet), vec![None, Some("doxology"), None]);
    }
}
