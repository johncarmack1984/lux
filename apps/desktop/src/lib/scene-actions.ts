import { createTauRPCProxy, type Scene } from "@/bindings";
import { queryClient } from "@/lib/query-client";
import { SCENES_QUERY_KEY } from "@/hooks/useScenes";
import { BUFFER_QUERY_KEY } from "@/hooks/useBuffer";
import { noteFadeStarted } from "@/lib/fade-clock";

/**
 * The Tauri adapter for scenes: every mutator lands the list the backend
 * returns straight in the query cache, the same cache-through pattern as
 * `lib/actions` and for the same reason — the `scenesSet` event never reaches
 * the webview on iOS, so the return value is the only reliable update.
 *
 * Recall is the odd one out and deliberately so: it returns nothing, because a
 * crossfade's first frame is stale before it arrives. The rig's truth is the
 * buffer, which `useBuffer` already watches.
 */
const cmd = () => createTauRPCProxy().cmd;

/** Land a mutator's returned list in the cache, cancelling any stale refetch. */
async function commit(scenes: Scene[]): Promise<Scene[]> {
  await queryClient.cancelQueries({ queryKey: SCENES_QUERY_KEY });
  queryClient.setQueryData(SCENES_QUERY_KEY, scenes);
  return scenes;
}

/** Save the live look as a new scene. */
export async function captureScene(name: string): Promise<Scene[]> {
  return commit(await cmd().capture_scene(name));
}

/** Re-capture an existing scene's levels from the live rig. */
export async function updateScene(id: string): Promise<Scene[]> {
  return commit(await cmd().update_scene(id));
}

/** Start the crossfade toward a scene. Resolves once the fade is running. */
export async function recallScene(id: string): Promise<void> {
  await cmd().recall_scene(id);
  // The fade now writes the buffer from the backend for its whole duration,
  // and on iOS no event will carry those ticks to the webview. Mark the fade
  // window and kick the buffer query so useBuffer polls until it closes —
  // otherwise the lights move and the faders stay frozen. An unknown fade
  // (cache miss) assumes the default 2 s rather than not following at all.
  const scenes = queryClient.getQueryData<Scene[]>(SCENES_QUERY_KEY);
  noteFadeStarted(scenes?.find((s) => s.id === id)?.fadeMs ?? 2000);
  void queryClient.invalidateQueries({ queryKey: BUFFER_QUERY_KEY });
}

export async function renameScene(id: string, name: string): Promise<Scene[]> {
  return commit(await cmd().rename_scene(id, name));
}

export async function setSceneFade(
  id: string,
  fadeMs: number,
): Promise<Scene[]> {
  return commit(await cmd().set_scene_fade(id, fadeMs));
}

/** Move a scene one place in either direction; the ends saturate. */
export async function moveScene(id: string, delta: number): Promise<Scene[]> {
  return commit(await cmd().move_scene(id, delta));
}

export async function deleteScene(id: string): Promise<Scene[]> {
  return commit(await cmd().delete_scene(id));
}
