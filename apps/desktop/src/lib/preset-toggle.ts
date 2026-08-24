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
 * The I/O is pluggable because the same buttons drive two different desks: the
 * owner's local buffer (the singleton below) and a guest's shared desk, whose
 * reads and writes cross the wire to the owner's rig. Each desk holds its own
 * store, so an owner's engaged presets and a guest desk's never see each other.
 */
export type PresetIo = {
  /** The last-known buffer, for the optimistic plan; undefined disables it. */
  cached(): number[] | undefined;
  /** A fresh read of the committed buffer, the base for the real write. */
  read(): Promise<number[]>;
  /** Write a full frame to the desk. */
  apply(frame: number[]): Promise<void>;
};

export type PresetStore = {
  toggle(id: string, writes: Map<number, number>, scope: PresetScope): Promise<void>;
  subscribe(listener: () => void): () => void;
  isActive(id: string): boolean;
  reconcile(buffer: number[]): void;
};

export function createPresetStore(io: PresetIo): PresetStore {
  /**
   * The active set is UI-side; the backend only ever sees buffer writes. Every
   * toggle starts from a fresh read so concurrent changes to unrelated
   * channels — another surface, the Discord bot — survive both engage and
   * restore.
   */
  let active: ActiveMap = new Map();
  const listeners = new Set<() => void>();

  const notify = () => {
    for (const listener of listeners) listener();
  };

  return {
    /**
     * Engage the preset `id` in `scope`, or toggle it off if it is already the
     * one engaged in that lane.
     *
     * The button highlights on the click, not on the commit: the press is
     * planned optimistically against the cached buffer so the toggle reads as
     * instant, then re-planned against a fresh read for the write that
     * actually lands. A failed write rolls the marker back, so the lights and
     * the toggle still end up agreeing.
     */
    async toggle(id, writes, scope) {
      const before = active;
      const cached = io.cached();
      if (cached) {
        active = planToggle(before, id, writes, scope, cached.slice()).next;
        notify();
      }
      try {
        const base = (await io.read()).slice();
        // Plan against the pre-click active set: the optimistic map above is
        // display-only, and planning against it would undo the preset twice.
        const { frame, next } = planToggle(before, id, writes, scope, base);
        await io.apply(frame);
        // Reconcile against the frame we just wrote so any preset this one
        // changed — e.g. Blackout when a fixture preset is engaged over it —
        // drops its marker in the same update, not a tick later when the
        // buffer read lands.
        active = reconcile(next, frame);
        notify();
      } catch (e) {
        active = before;
        notify();
        throw e;
      }
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    isActive: (id) => isPresetActive(active, id),
    reconcile(buffer) {
      const next = reconcile(active, buffer);
      if (next !== active) {
        active = next;
        notify();
      }
    },
  };
}

/**
 * The owner's desk: reads and writes the local buffer, cache-through like
 * lib/actions because the `bufferSet` event never reaches the webview on iOS.
 */
export const ownerPresetStore = createPresetStore({
  cached: () => queryClient.getQueryData<number[]>(BUFFER_QUERY_KEY),
  read: async () => (await createTauRPCProxy().sync.sync_buffer()).buffer,
  async apply(frame) {
    const committed = await createTauRPCProxy().cmd.set_buffer(frame);
    await queryClient.cancelQueries({ queryKey: BUFFER_QUERY_KEY });
    queryClient.setQueryData(BUFFER_QUERY_KEY, committed.buffer);
  },
});

/** Whether a preset with this `id` is engaged. Re-renders on any change. */
export function usePresetActive(store: PresetStore, id: string): boolean {
  return useSyncExternalStore(store.subscribe, () => store.isActive(id));
}

/**
 * Drop any active preset whose look the live buffer no longer shows (see
 * `reconcile`). Mount next to the desk's buffer read (PresetRow does, on every
 * surface) — running it from several components is harmless.
 */
export function usePresetReconcile(store: PresetStore, buffer: number[] | null) {
  useEffect(() => {
    if (!buffer) return;
    store.reconcile(buffer);
  }, [store, buffer]);
}
