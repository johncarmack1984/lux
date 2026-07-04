import { useQuery } from "@tanstack/react-query";
import { createTauRPCProxy, type SyncState } from "@/bindings";

/** Query key for the cloud-sync state. */
export const SYNC_STATUS_QUERY_KEY = ["syncStatus"] as const;

/**
 * The current cloud-sync state (idle / syncing / synced / offline). `null` until
 * the first read.
 *
 * The backend pushes this as a push/pull cycle progresses via the
 * `syncStatusChanged` event, which doesn't reach the webview on iOS. It's driven
 * by the backend rather than a UI mutation, so poll it — `sync_status` is a
 * cheap in-memory read, the poll pauses while the app is backgrounded, and the
 * indicator is only mounted while signed in.
 */
export default function useSyncStatus(): SyncState | null {
  const { data } = useQuery({
    queryKey: SYNC_STATUS_QUERY_KEY,
    queryFn: () => createTauRPCProxy().cmd.sync_status(),
    refetchInterval: 2000,
  });
  return data ?? null;
}
