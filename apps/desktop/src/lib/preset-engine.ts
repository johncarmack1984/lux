//! The preset state machine, as pure data — no React, no Tauri, no buffer I/O.
//!
//! Presets are momentary looks, not destinations: engaging one remembers the
//! channels it replaces, and toggling it off puts them back. Two kinds share
//! this machine, and the whole point of the scoping here is that they don't
//! step on each other:
//!
//!   - **Full-setup** presets (Blackout, Full Bright) write the entire universe.
//!     They share a single lane (`{ kind: "setup" }`): engaging one replaces the
//!     other, exactly as before.
//!   - **Fixture** presets (Reading Light) write one fixture's channels. Each
//!     fixture is its own lane (`{ kind: "fixture", fixtureId }`), so Reading
//!     Light on fixture A and on fixture B are independent — engaging one never
//!     disturbs the other's channels or its active marker. Fixtures own disjoint
//!     address ranges (the patch validator guarantees it), so fixture lanes
//!     never collide.
//!
//! **Deactivation is strict and per-preset.** A preset is dropped the moment any
//! channel it wrote no longer matches — whether the change came from a fader, a
//! remote command, or another preset. So engaging Reading Light on fixture A
//! while Blackout is up toggles Blackout *off* (Blackout's channels on fixture A
//! changed); the fixtures the fixture preset didn't touch keep the value
//! Blackout left them at, because reconcile only clears the marker — it never
//! restores. The one way to get a preset's remembered frame back is to toggle
//! that preset off from its own button (see `planToggle`), which is only
//! possible while it is still active, i.e. before anything else has changed it.
//!
//! Everything here is pure: inputs are copied, never mutated, so the same call
//! is safe to make from a React render or a test. The active set is UI-side
//! state; the backend only ever sees the frames these functions produce.

/** Which lane a preset lives in. Setup presets share one lane; fixtures each own one. */
export type PresetScope =
  | { kind: "setup" }
  | { kind: "fixture"; fixtureId: string };

/** The map key for a lane. Setup collapses to one key; fixtures key by id. */
export function scopeKey(scope: PresetScope): string {
  return scope.kind === "setup" ? "setup" : `fixture:${scope.fixtureId}`;
}

export type ActivePreset = {
  id: string;
  scope: PresetScope;
  /** Prior value of each address the preset wrote (restore target). */
  snapshot: Map<number, number>;
  /** What the preset wrote (1-based address → value); divergence deactivates. */
  expected: Map<number, number>;
};

/** The engaged presets, keyed by lane (`scopeKey`). At most one per lane. */
export type ActiveMap = ReadonlyMap<string, ActivePreset>;

/**
 * Plan engaging preset `id` in `scope`, or toggling it off if it is already the
 * one engaged in that lane. Returns the frame to commit and the next active map.
 *
 * Only the target lane is touched: the outgoing preset *in that lane* (if any)
 * is undone first, so switching within a lane composes undo + engage into one
 * frame and toggling off always returns to a state the user actually set. Other
 * lanes — and their contributions to `base` — are left exactly as they are, so
 * a fixture preset never disturbs another fixture or the full-setup look.
 */
export function planToggle(
  active: ActiveMap,
  id: string,
  writes: Map<number, number>,
  scope: PresetScope,
  base: number[],
): { frame: number[]; next: ActiveMap } {
  const key = scopeKey(scope);
  const prev = active.get(key) ?? null;

  // Start from the live frame and undo only this lane's outgoing preset.
  const frame = base.slice();
  for (const [addr, value] of prev?.snapshot ?? []) frame[addr - 1] = value;

  const next = new Map(active);
  if (prev?.id === id) {
    // Same preset in this lane → toggle it off; its channels are already restored.
    next.delete(key);
    return { frame, next };
  }

  // Engage: snapshot the addresses we're about to write (post-undo, so restore
  // returns to "this lane absent"), then lay the preset down.
  const snapshot = new Map<number, number>();
  for (const addr of writes.keys()) snapshot.set(addr, frame[addr - 1] ?? 0);
  for (const [addr, value] of writes) frame[addr - 1] = value;
  next.set(key, { id, scope, snapshot, expected: writes });
  return { frame, next };
}

/**
 * Drop any preset whose look the live `buffer` no longer shows: a preset is
 * cleared the moment one of the channels it wrote diverges, whatever moved it —
 * a fader, a remote command, or another preset engaged on top. This clears the
 * *marker* only; it never writes, so the divergent state stands (that's what
 * makes "engage Blackout, then raise one fixture" leave the rest at 0). Because
 * each preset watches only its own channels, a change to one fixture never
 * clears another's marker. Returns the same map reference when nothing is
 * dropped, so callers can skip a re-render on identity.
 */
export function reconcile(active: ActiveMap, buffer: number[]): ActiveMap {
  const drop: string[] = [];
  for (const [key, p] of active) {
    for (const [addr, value] of p.expected) {
      if (buffer[addr - 1] !== value) {
        drop.push(key);
        break;
      }
    }
  }

  if (drop.length === 0) return active;
  const next = new Map(active);
  for (const key of drop) next.delete(key);
  return next;
}

/** Whether a preset with this `id` is engaged in any lane. */
export function isPresetActive(active: ActiveMap, id: string): boolean {
  for (const p of active.values()) if (p.id === id) return true;
  return false;
}
