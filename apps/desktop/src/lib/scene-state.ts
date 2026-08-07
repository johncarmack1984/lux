//! Which saved look is on the rig right now — as pure data, no React, no Tauri.
//!
//! A scene is a sparse set of levels (see `crates/../scene.rs`), so "this scene
//! is up" means exactly: every slot the scene names still reads the value it
//! saved. Slots the scene never mentioned are none of its business — that is
//! the same sparse-overlay contract the backend recall honours, restated for
//! the button's pressed state.
//!
//! Deliberately *not* the preset engine's job. Presets are momentary and carry
//! an undo lane, so they need a remembered active set that survives an
//! unrelated write (`lib/preset-engine.ts`). A scene is a destination: whether
//! it is showing is answerable from the buffer alone, with nothing remembered,
//! which is why this is a function rather than a store.

import type { Scene } from "@/bindings";

/** Whether the live `buffer` shows every level this scene saved. */
export function isSceneShowing(buffer: number[], scene: Scene): boolean {
  // An empty scene matches nothing: the backend refuses to save one, and
  // "matches everything" would be a permanently-lit button.
  if (scene.levels.length === 0) return false;
  return scene.levels.every(({ ch, val }) => buffer[ch - 1] === val);
}

/**
 * The scene the rig is currently showing, or `null`.
 *
 * First match wins on the rare tie — two scenes saved with identical levels are
 * the same look, so highlighting the earlier one is both stable and honest.
 * `null` while the buffer is still loading, so a button never flashes pressed
 * before the first read lands.
 */
export function activeSceneId(
  buffer: number[] | null,
  scenes: Scene[] | null,
): string | null {
  if (!buffer || !scenes) return null;
  return scenes.find((scene) => isSceneShowing(buffer, scene))?.id ?? null;
}
