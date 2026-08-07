import { test, expect, describe } from "bun:test";
import type { Scene } from "@/bindings";
import { activeSceneId, isSceneShowing } from "@/lib/scene-state";

/** A scene over a small stand-in universe, addressed 1-based like DMX. */
const scene = (id: string, levels: Record<number, number>): Scene => ({
  id,
  name: id,
  levels: Object.entries(levels).map(([ch, val]) => ({
    ch: Number(ch),
    val,
  })),
  fadeMs: 2000,
});

// Fixture A owns slots 1..3, fixture B owns 4..6 — the patch validator
// guarantees they are disjoint, exactly as the preset engine's tests assume.
const WORSHIP = scene("worship", { 1: 255, 2: 128, 3: 0 });
const SERMON = scene("sermon", { 1: 40, 2: 40, 3: 40 });

describe("isSceneShowing", () => {
  test("matches only the slots the scene named", () => {
    // Slots 4..6 belong to another fixture and moved since capture; the scene
    // is still showing, because a sparse scene never claimed them.
    expect(isSceneShowing([255, 128, 0, 99, 99, 99], WORSHIP)).toBe(true);
  });

  test("one divergent slot is enough to drop it", () => {
    expect(isSceneShowing([255, 127, 0, 0, 0, 0], WORSHIP)).toBe(false);
  });

  test("mid-fade is not showing", () => {
    // Halfway through a recall the buffer holds interpolated values — the
    // button lights up when the fade lands, not when it starts.
    expect(isSceneShowing([128, 64, 0, 0, 0, 0], WORSHIP)).toBe(false);
  });

  test("a scene naming a slot past the universe never matches", () => {
    expect(isSceneShowing([255], scene("wide", { 1: 255, 900: 10 }))).toBe(
      false,
    );
  });

  test("an empty scene matches nothing", () => {
    expect(isSceneShowing([0, 0, 0], scene("empty", {}))).toBe(false);
  });
});

describe("activeSceneId", () => {
  test("finds the scene the buffer is showing", () => {
    const scenes = [WORSHIP, SERMON];
    expect(activeSceneId([255, 128, 0, 0, 0, 0], scenes)).toBe("worship");
    expect(activeSceneId([40, 40, 40, 0, 0, 0], scenes)).toBe("sermon");
  });

  test("is null when the rig shows no saved look", () => {
    expect(activeSceneId([1, 2, 3, 4, 5, 6], [WORSHIP, SERMON])).toBeNull();
    expect(activeSceneId([255, 128, 0], [])).toBeNull();
  });

  test("is null while either read is still in flight", () => {
    // Nothing is pressed before the first buffer read lands.
    expect(activeSceneId(null, [WORSHIP])).toBeNull();
    expect(activeSceneId([255, 128, 0], null)).toBeNull();
  });

  test("a tie resolves to the first scene in display order", () => {
    // Two scenes saved with the same levels are the same look; highlighting
    // the earlier one is stable across reorders of everything else.
    const twin = { ...WORSHIP, id: "twin" };
    expect(activeSceneId([255, 128, 0], [WORSHIP, twin])).toBe("worship");
  });
});
