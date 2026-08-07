import { useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { createTauRPCProxy, type Scene } from "@/bindings";

/** Query key for the active setup's saved looks. */
export const SCENES_QUERY_KEY = ["scenes"] as const;

/**
 * The active setup's scenes, in display order. `null` while the first read is
 * in flight.
 *
 * Same shape as `useFixtures`, for the same reason: the backend emits a
 * `scenesSet` event on every capture/rename/reorder/delete and after a cloud
 * pull, but that event never reaches the webview on iOS — so the mutators push
 * the list they return straight into this cache (see `lib/scene-actions`), and
 * the event is honoured only as a desktop fast path for out-of-band changes.
 */
export default function useScenes(): Scene[] | null {
  const queryClient = useQueryClient();

  const { data } = useQuery({
    queryKey: SCENES_QUERY_KEY,
    queryFn: () => createTauRPCProxy().cmd.list_scenes(),
  });

  useEffect(() => {
    const unlisten = createTauRPCProxy().cmd.event.on((event) => {
      if (event.type === "scenesSet") {
        queryClient.setQueryData(SCENES_QUERY_KEY, event.scenes);
      }
    });
    return () => {
      // .catch: if registration itself rejected (webview teardown), cleanup
      // must not surface an unhandled rejection.
      unlisten.then((f) => f()).catch(() => {});
    };
  }, [queryClient]);

  return data ?? null;
}
