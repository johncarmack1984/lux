//! User-level preferences: one small [`UserSettings`] blob per user, persisted
//! in the [`crate::setup::SetupStore`] and cloud-synced whole (last-writer-wins
//! by server timestamp, like a setup). The pure merge decision lives here as
//! [`reconcile`]; [`crate::setup::LuxSetups::merge_remote_settings`] applies it
//! atomically under the store lock.
//!
//! On the wire the blob is opaque — the sync-api round-trips it as JSON
//! (`lux_wire::SettingsRecord::data`) and only this crate types it — so adding
//! a setting here never requires a server deploy. To keep that evolution safe
//! in both directions, every field carries
//! `#[serde(default, deserialize_with = "ok_or_default")]`: a missing field
//! (older client's blob on a newer one) and an unreadable *value* (a newer
//! client's enum variant, or a corrupted write, on an older one) both degrade
//! to that field's default instead of failing the whole blob — and with it the
//! whole `setups.json` — on a downgrade. Unknown fields are ignored as usual.
//! The tests at the bottom pin all of this.

use lux_wire::SettingsRecord;
use serde::{Deserialize, Serialize};
use specta::Type;

/// Which way the universe desk's faders run: vertical sliders in a
/// horizontally-scrolling desk (the classic lighting-console layout, and the
/// default), or horizontal sliders in a vertically-scrolling list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SliderOrientation {
    #[default]
    Vertical,
    Horizontal,
}

/// The user's synced preferences. Add fields with
/// `#[serde(default, deserialize_with = "ok_or_default")]` only — see the
/// module docs for why.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UserSettings {
    // specta(type): ok_or_default doesn't change the wire type — it only
    // degrades unreadable input — so pin the export to the field's own type.
    #[serde(default, deserialize_with = "ok_or_default")]
    #[specta(type = SliderOrientation)]
    pub slider_orientation: SliderOrientation,
}

/// Field-level fault tolerance: a present-but-unreadable value (an enum
/// variant from a newer client, a wrong type from a corrupted write) degrades
/// to the field's default instead of failing the containing document's parse.
/// (`#[serde(default)]` alone only covers a *missing* field.)
fn ok_or_default<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: serde::de::DeserializeOwned + Default,
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(T::deserialize(value).unwrap_or_default())
}

// --- cloud merge (pure; applied under the store lock by LuxSetups) ----------

/// What a pull decided about the settings blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Merge {
    /// Local state already agrees with (or outranks) the cloud — change nothing.
    KeepLocal,
    /// The cloud's blob wins (LWW): adopt these values at this server timestamp.
    Adopt(UserSettings, i64),
    /// Keep the local values but advance the concurrency base to this server
    /// timestamp, so the next push lands as a conditional overwrite instead of
    /// a doomed create. Used when local outranks remote (a claim) or when the
    /// remote blob won't parse (a dirty push then repairs it).
    Rebase(i64),
}

/// Decide whether the cloud's settings blob should replace the local one — the
/// same last-writer-wins-by-server-`updatedAt` rule as the setups `reconcile`,
/// applied to the account's single settings record.
///
/// The one deliberate divergence from plain LWW: a **dirty local edit that was
/// never synced** (`base` = `None`) outranks whatever the account has stored —
/// an explicit choice made seconds ago on this device must not silently revert
/// to a record written months ago elsewhere. It rebases onto the remote
/// timestamp so its claim push succeeds conditionally.
pub fn reconcile(base: Option<i64>, dirty: bool, remote: Option<&SettingsRecord>) -> Merge {
    let Some(r) = remote else {
        // Nothing in the cloud yet; a claim push (if pending) creates it.
        return Merge::KeepLocal;
    };
    let parsed = serde_json::from_value::<UserSettings>(r.data.clone()).ok();
    match base {
        // Never synced on this device (fresh install, account switch, or
        // post-account-deletion): a clean store adopts the account's record; a
        // dirty edit claims over it; an unparseable record just hands over its
        // timestamp (no adoption, no wedge — see Merge::Rebase).
        None => match (dirty, parsed) {
            (false, Some(settings)) => Merge::Adopt(settings, r.updated_at),
            _ => Merge::Rebase(r.updated_at),
        },
        Some(base) => {
            if r.updated_at <= base {
                // Remote is our own base (or older) — local state stands; a
                // dirty edit re-pushes on the base it already holds.
                Merge::KeepLocal
            } else {
                // Remote is newer: LWW gives it the win, even over a dirty
                // local edit (mirrors the setups rule). If it won't parse,
                // take only its timestamp so a dirty push can repair it.
                match parsed {
                    Some(settings) => Merge::Adopt(settings, r.updated_at),
                    None => Merge::Rebase(r.updated_at),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_to_vertical() {
        assert_eq!(
            UserSettings::default().slider_orientation,
            SliderOrientation::Vertical
        );
    }

    #[test]
    fn json_shape_is_pinned() {
        // This exact JSON crosses the wire inside `SettingsRecord::data` and is
        // what older/newer clients parse of each other — a change here is a
        // compatibility decision, not a refactor.
        assert_eq!(
            serde_json::to_string(&UserSettings::default()).unwrap(),
            r#"{"sliderOrientation":"vertical"}"#
        );
        let horizontal: UserSettings =
            serde_json::from_str(r#"{"sliderOrientation":"horizontal"}"#).unwrap();
        assert_eq!(
            horizontal.slider_orientation,
            SliderOrientation::Horizontal
        );
    }

    #[test]
    fn tolerates_missing_and_unknown_fields() {
        // Older client's blob (field absent) → default.
        let empty: UserSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.slider_orientation, SliderOrientation::Vertical);

        // Newer client's blob (extra field) → still parses.
        let future: UserSettings =
            serde_json::from_str(r#"{"sliderOrientation":"horizontal","theme":"dark"}"#).unwrap();
        assert_eq!(future.slider_orientation, SliderOrientation::Horizontal);
    }

    #[test]
    fn unreadable_values_degrade_to_default() {
        // A future client writes a variant this build doesn't know. The
        // `ok_or_default` fallback keeps the blob — and the whole setups.json
        // it is embedded in — parseable on a downgrade.
        let future: UserSettings =
            serde_json::from_str(r#"{"sliderOrientation":"grid"}"#).unwrap();
        assert_eq!(future.slider_orientation, SliderOrientation::Vertical);

        // So does a wrong-typed value from a corrupted write.
        let corrupt: UserSettings =
            serde_json::from_str(r#"{"sliderOrientation":42}"#).unwrap();
        assert_eq!(corrupt.slider_orientation, SliderOrientation::Vertical);
    }

    // -- reconcile --

    fn record(data: serde_json::Value, updated_at: i64) -> SettingsRecord {
        SettingsRecord {
            data,
            rev: 1,
            updated_at,
        }
    }

    fn horizontal(updated_at: i64) -> SettingsRecord {
        record(json!({ "sliderOrientation": "horizontal" }), updated_at)
    }

    #[test]
    fn no_remote_keeps_local() {
        assert_eq!(reconcile(None, false, None), Merge::KeepLocal);
        assert_eq!(reconcile(Some(100), true, None), Merge::KeepLocal);
    }

    #[test]
    fn first_pull_of_a_clean_store_adopts_remote() {
        let r = horizontal(100);
        assert_eq!(
            reconcile(None, false, Some(&r)),
            Merge::Adopt(
                UserSettings {
                    slider_orientation: SliderOrientation::Horizontal
                },
                100
            )
        );
    }

    #[test]
    fn never_synced_dirty_edit_claims_over_the_stored_record() {
        // An explicit edit made before first sync outranks the account's old
        // record; rebasing lets its push land conditionally instead of 409ing.
        let r = horizontal(100);
        assert_eq!(reconcile(None, true, Some(&r)), Merge::Rebase(100));
    }

    #[test]
    fn remote_newer_wins_even_over_dirty() {
        let r = horizontal(200);
        let adopt = Merge::Adopt(
            UserSettings {
                slider_orientation: SliderOrientation::Horizontal,
            },
            200,
        );
        assert_eq!(reconcile(Some(100), false, Some(&r)), adopt);
        assert_eq!(reconcile(Some(100), true, Some(&r)), adopt);
    }

    #[test]
    fn dirty_on_latest_base_is_kept_for_repush() {
        let r = horizontal(100);
        assert_eq!(reconcile(Some(100), true, Some(&r)), Merge::KeepLocal);
    }

    #[test]
    fn in_sync_is_a_no_op() {
        let r = record(json!({ "sliderOrientation": "vertical" }), 100);
        assert_eq!(reconcile(Some(100), false, Some(&r)), Merge::KeepLocal);
    }

    #[test]
    fn unparseable_remote_rebases_instead_of_wedging() {
        // Garbage in the cloud (any writer bug): never adopt it, but take its
        // timestamp so a dirty local push conditionally overwrites — without
        // this, a never-synced claim 409s forever and sync sticks at Offline.
        let bad = record(json!("not an object"), 200);
        assert_eq!(reconcile(None, false, Some(&bad)), Merge::Rebase(200));
        assert_eq!(reconcile(None, true, Some(&bad)), Merge::Rebase(200));
        assert_eq!(reconcile(Some(100), true, Some(&bad)), Merge::Rebase(200));
        // In-sync-or-older garbage changes nothing.
        assert_eq!(reconcile(Some(200), false, Some(&bad)), Merge::KeepLocal);
    }
}
