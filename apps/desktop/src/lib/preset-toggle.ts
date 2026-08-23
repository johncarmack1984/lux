import { useEffect, useSyncExternalStore } from "react";
import { createTauRPCProxy } from "@/bindings";
import { queryClient } from "@/lib/query-client";
import { BUFFER_QUERY_KEY } from "@/hooks/useBuffer";
import {
  planToggle,
  reconcile,
  isPresetActive,
  type ActiveMap,
  type PresetScope,
} from "@/lib/preset-engine";

/**
 * The React + Tauri adapter around the pure preset engine (`preset-engine.ts`):
 * it holds the live active set, drives it from real buffer reads/writes, and
 * exposes the store hooks the UI subscribes to. All the scoping and layering
 * rules — why a fixture preset never disturbs another fixture, why a full-setup
 * marker survives a layered fixture — live in the engine and its tests; this
 * file only does I/O.
 *
 * The active set is UI-side; the backend only ever sees buffer writes. Every
 * toggle starts from a fresh backend read so concurrent changes to unrelated
 * channels — another surface, the Discord bot — survive both engage and restore.
 */
let active: ActiveMap = new Map();
const listeners = new Set<() => void>();

function notify() {
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * Write a full frame and land the committed buffer in the query cache — the
 * same cache-through pattern as lib/actions, because the `bufferSet` event
 * never reaches the webview on iOS.
 */
async function applyBuffer(frame: number[]) {
  const committed = await createTauRPCProxy().cmd.set_buffer(frame);
  await queryClient.cancelQueries({ queryKey: BUFFER_QUERY_KEY });
  queryClient.setQueryData(BUFFER_QUERY_KEY, committed.buffer);
}

/**
 * Engage the preset `id` in `scope`, or toggle it off if it is already the one
 * engaged in that lane.
 *
 * The button highlights on the click, not on the commit: the press is planned
 * optimistically against the cached buffer so the toggle reads as instant,
 * then re-planned against a fresh backend read for the write that actually
 * lands — concurrent changes to unrelated channels (another surface, the
 * Discord bot) survive both engage and restore. A failed write rolls the
 * marker back, so the lights and the toggle still end up agreeing.
 */
export async function togglePreset(
  id: string,
  writes: Map<number, number>,
  scope: PresetScope,
) {
  const before = active;
  const cached = queryClient.getQueryData<number[]>(BUFFER_QUERY_KEY);
  if (cached) {
    active = planToggle(before, id, writes, scope, cached.slice()).next;
    notify();
  }
  try {
    const base = (await createTauRPCProxy().sync.sync_buffer()).buffer.slice();
    // Plan against the pre-click active set: the optimistic map above is
    // display-only, and planning against it would undo the preset twice.
    const { frame, next } = planToggle(before, id, writes, scope, base);
    await applyBuffer(frame);
    // Reconcile against the frame we just wrote so any preset this one changed —
    // e.g. Blackout when a fixture preset is engaged over it — drops its marker
    // in the same update, not a tick later when the buffer read lands.
    active = reconcile(next, frame);
    notify();
  } catch (e) {
    active = before;
    notify();
    throw e;
  }
}

/** Whether a preset with this `id` is engaged. Re-renders on any change. */
export function useIsPresetActive(id: string): boolean {
  return useSyncExternalStore(subscribe, () => isPresetActive(active, id));
}

/**
 * Drop any active preset whose look the live buffer no longer shows (see
 * `reconcile`). Mount next to a `useBuffer()` read (ButtonRow does, on both
 * surfaces) — running it from several components is harmless.
 */
export function usePresetReconcile(buffer: number[] | null) {
  useEffect(() => {
    if (!buffer) return;
    const next = reconcile(active, buffer);
    if (next !== active) {
      active = next;
      notify();
    }
  }, [buffer]);
}
