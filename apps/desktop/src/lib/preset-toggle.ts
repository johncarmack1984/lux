import { useEffect, useSyncExternalStore } from "react";
import { createTauRPCProxy } from "@/bindings";
import { queryClient } from "@/lib/query-client";
import { BUFFER_QUERY_KEY } from "@/hooks/useBuffer";

/**
 * Presets are momentary looks, not destinations: engaging one remembers the
 * channels it replaces, and toggling it off puts them back. The active preset
 * is a UI-side notion — the backend only ever sees buffer writes — so the
 * store also watches the live buffer and quietly drops the active state when
 * anything else (a fader, the wheel, a remote command) moves a channel the
 * preset set, rather than offering a restore that would clobber it.
 *
 * Snapshots are sparse (only the addresses the preset wrote) and every toggle
 * starts from a fresh backend read, so concurrent changes to unrelated
 * channels — another surface, the Discord bot — survive both engage and
 * restore.
 */
type ActivePreset = {
  id: string;
  /** Prior value of each address the preset wrote (restore target). */
  snapshot: Map<number, number>;
  /** What the preset wrote (1-based address → value); divergence deactivates. */
  expected: Map<number, number>;
};

let active: ActivePreset | null = null;
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
 * Engage the preset `id`, or toggle it off if it is already the active one.
 *
 * Either way the outgoing preset (if any) is undone first, so switching
 * directly between presets composes undo + engage into a single frame:
 * toggling off always returns to a state the user actually set, never to
 * another preset's output. The active mark is only set once the backend
 * commits, so a failed write leaves both the lights and the toggle untouched.
 */
export async function togglePreset(id: string, writes: Map<number, number>) {
  const previous = active;
  const base = (await createTauRPCProxy().sync.sync_buffer()).buffer.slice();
  for (const [address, value] of previous?.snapshot ?? []) {
    base[address - 1] = value;
  }
  if (previous?.id === id) {
    active = null;
    notify();
    await applyBuffer(base);
    return;
  }
  const snapshot = new Map<number, number>();
  for (const address of writes.keys()) {
    snapshot.set(address, base[address - 1] ?? 0);
  }
  const frame = base.slice();
  for (const [address, value] of writes) frame[address - 1] = value;
  await applyBuffer(frame);
  active = { id, snapshot, expected: writes };
  notify();
}

/** The id of the engaged preset, or null. Re-renders on engage/clear. */
export function useActivePresetId(): string | null {
  return useSyncExternalStore(subscribe, () => active?.id ?? null);
}

/**
 * Drop the active preset when the live buffer no longer matches what it
 * wrote. Mount next to a `useBuffer()` read (ButtonRow does, on both
 * surfaces) — running it from several components is harmless.
 */
export function usePresetReconcile(buffer: number[] | null) {
  useEffect(() => {
    if (!active || !buffer) return;
    for (const [address, value] of active.expected) {
      if (buffer[address - 1] !== value) {
        active = null;
        notify();
        return;
      }
    }
  }, [buffer]);
}
