import { test, expect, describe } from "bun:test";
import {
  planToggle,
  reconcile,
  isPresetActive,
  scopeKey,
  type ActiveMap,
  type PresetScope,
} from "@/lib/preset-engine";

// A small stand-in universe (6 channels) keeps the fixtures below readable.
// Fixture A owns 1..3, fixture B owns 4..6 — disjoint, as the patch validator
// guarantees. A "full-setup" preset writes the whole thing.
const SETUP: PresetScope = { kind: "setup" };
const FIX_A: PresetScope = { kind: "fixture", fixtureId: "A" };
const FIX_B: PresetScope = { kind: "fixture", fixtureId: "B" };

/** Build a sparse address→value map from a plain object. */
const w = (o: Record<number, number>): Map<number, number> =>
  new Map(Object.entries(o).map(([k, v]) => [Number(k), v]));

const READING_A = w({ 1: 255, 2: 128, 3: 10 });
const READING_B = w({ 4: 255, 5: 128, 6: 10 });
const BLACKOUT = w({ 1: 0, 2: 0, 3: 0, 4: 0, 5: 0, 6: 0 });
const FULL = w({ 1: 255, 2: 255, 3: 255, 4: 255, 5: 255, 6: 255 });

const EMPTY: ActiveMap = new Map();
const zeros = () => [0, 0, 0, 0, 0, 0];

/** Apply a toggle and hand back the resulting frame + active map together. */
function toggle(
  active: ActiveMap,
  id: string,
  writes: Map<number, number>,
  scope: PresetScope,
  base: number[],
) {
  const { frame, next } = planToggle(active, id, writes, scope, base);
  return { frame, active: next };
}

describe("fixture presets are independent per fixture", () => {
  test("engaging a preset on B leaves A's marker and A's channels untouched", () => {
    let frame = zeros();
    let active = EMPTY;

    ({ frame, active } = toggle(active, "reading-A", READING_A, FIX_A, frame));
    ({ frame, active } = toggle(active, "reading-B", READING_B, FIX_B, frame));

    // Both markers stay lit — this is the bug John reported.
    expect(isPresetActive(active, "reading-A")).toBe(true);
    expect(isPresetActive(active, "reading-B")).toBe(true);

    // A's channels still carry the reading light; B's now do too.
    expect(frame.slice(0, 3)).toEqual([255, 128, 10]);
    expect(frame.slice(3, 6)).toEqual([255, 128, 10]);
  });

  test("toggling B off restores only B and never disturbs A", () => {
    let frame = zeros();
    let active = EMPTY;
    ({ frame, active } = toggle(active, "reading-A", READING_A, FIX_A, frame));
    ({ frame, active } = toggle(active, "reading-B", READING_B, FIX_B, frame));

    // Press B again to release it.
    ({ frame, active } = toggle(active, "reading-B", READING_B, FIX_B, frame));

    expect(isPresetActive(active, "reading-A")).toBe(true);
    expect(isPresetActive(active, "reading-B")).toBe(false);
    expect(frame.slice(0, 3)).toEqual([255, 128, 10]); // A untouched
    expect(frame.slice(3, 6)).toEqual([0, 0, 0]); // B back to its snapshot
  });

  test("each fixture snapshots its own prior state independently", () => {
    // A starts at a manual look, B at zero.
    const base = [50, 60, 70, 0, 0, 0];
    let frame = base.slice();
    let active = EMPTY;
    ({ frame, active } = toggle(active, "reading-A", READING_A, FIX_A, frame));
    ({ frame, active } = toggle(active, "reading-B", READING_B, FIX_B, frame));
    // Release A — it must return to A's manual look, not to zero.
    ({ frame } = toggle(active, "reading-A", READING_A, FIX_A, frame));
    expect(frame.slice(0, 3)).toEqual([50, 60, 70]);
    expect(frame.slice(3, 6)).toEqual([255, 128, 10]); // B still lit
  });
});

describe("a fixture change toggles the full-setup preset off (John's rule)", () => {
  test("engaging a fixture preset over Blackout toggles Blackout off; other fixtures stay at 0", () => {
    let frame = zeros();
    let active = EMPTY;
    ({ frame, active } = toggle(active, "blackout", BLACKOUT, SETUP, frame));
    ({ frame, active } = toggle(active, "reading-A", READING_A, FIX_A, frame));

    // Reconcile is what runs off the buffer write in the app. Blackout wrote
    // fixture A's channels; the reading light changed them, so Blackout drops.
    active = reconcile(active, frame);
    expect(isPresetActive(active, "blackout")).toBe(false);
    expect(isPresetActive(active, "reading-A")).toBe(true);
    // Fixtures 2/3 (addresses 4..6) keep the 0 Blackout left — never restored.
    expect(frame).toEqual([255, 128, 10, 0, 0, 0]);
  });

  test("moving a single fader while Blackout is up toggles Blackout off, rest unchanged", () => {
    let frame = zeros();
    let active = EMPTY;
    ({ frame, active } = toggle(active, "blackout", BLACKOUT, SETUP, frame));
    // A fader nudge on one channel (fixture B here).
    frame[4] = 90;
    active = reconcile(active, frame);
    expect(isPresetActive(active, "blackout")).toBe(false);
    expect(frame).toEqual([0, 0, 0, 0, 90, 0]); // reconcile never restores
  });

  test("a full-setup preset overrides everything and clears fixture markers", () => {
    let frame = zeros();
    let active = EMPTY;
    ({ frame, active } = toggle(active, "reading-A", READING_A, FIX_A, frame));
    ({ frame, active } = toggle(active, "blackout", BLACKOUT, SETUP, frame));
    active = reconcile(active, frame);
    expect(frame).toEqual([0, 0, 0, 0, 0, 0]);
    expect(isPresetActive(active, "blackout")).toBe(true);
    expect(isPresetActive(active, "reading-A")).toBe(false);
  });

  test("two full-setup presets share one lane — the second replaces the first", () => {
    let frame = zeros();
    let active = EMPTY;
    ({ frame, active } = toggle(active, "blackout", BLACKOUT, SETUP, frame));
    ({ frame, active } = toggle(active, "full-bright", FULL, SETUP, frame));
    expect(isPresetActive(active, "blackout")).toBe(false);
    expect(isPresetActive(active, "full-bright")).toBe(true);
    expect(frame).toEqual([255, 255, 255, 255, 255, 255]);
  });
});

describe("toggling a preset off from its button restores the prior frame", () => {
  test("Blackout on, then off (untouched) returns the lights to their prior state", () => {
    // A live look before anyone touches Blackout.
    const prior = [10, 20, 30, 40, 50, 60];
    let frame = prior.slice();
    let active = EMPTY;
    ({ frame, active } = toggle(active, "blackout", BLACKOUT, SETUP, frame));
    expect(frame).toEqual([0, 0, 0, 0, 0, 0]);
    expect(isPresetActive(active, "blackout")).toBe(true);

    // "Several hours later" — nothing else moved a channel, so Blackout is
    // still active — pressing it again restores exactly the prior look.
    ({ frame, active } = toggle(active, "blackout", BLACKOUT, SETUP, frame));
    expect(frame).toEqual(prior);
    expect(isPresetActive(active, "blackout")).toBe(false);
  });
});

describe("reconcile mechanics", () => {
  test("an external change to a fixture's channel deactivates that fixture's preset only", () => {
    let frame = zeros();
    let active = EMPTY;
    ({ frame, active } = toggle(active, "reading-A", READING_A, FIX_A, frame));
    ({ frame, active } = toggle(active, "reading-B", READING_B, FIX_B, frame));

    // A fader (or remote) nudges one of A's channels away from what A wrote.
    frame[0] = 200;
    active = reconcile(active, frame);
    expect(isPresetActive(active, "reading-A")).toBe(false);
    expect(isPresetActive(active, "reading-B")).toBe(true);
  });

  test("reconcile returns the same reference when nothing is dropped", () => {
    let frame = zeros();
    let active = EMPTY;
    ({ frame, active } = toggle(active, "reading-A", READING_A, FIX_A, frame));
    expect(reconcile(active, frame)).toBe(active);
  });
});

describe("scopeKey", () => {
  test("setup collapses to one lane; fixtures key by id", () => {
    expect(scopeKey(SETUP)).toBe(scopeKey({ kind: "setup" }));
    expect(scopeKey(FIX_A)).not.toBe(scopeKey(FIX_B));
    expect(scopeKey(FIX_A)).toBe(scopeKey({ kind: "fixture", fixtureId: "A" }));
  });
});
