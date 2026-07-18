import { useQuery } from "@tanstack/react-query";
import { createTauRPCProxy, type RemotePeer } from "@/bindings";

/** Query key for the user's other live devices on the user channel. */
export const REMOTE_PEERS_QUERY_KEY = ["remotePeers"] as const;

/**
 * The user's other signed-in devices, learned from their retained presence
 * cards on the user channel. `null` until the first read.
 *
 * Backend-driven (cards arrive over MQTT, not from UI mutations), so this
 * polls like `useSyncStatus` — `list_remote_peers` is a cheap in-memory read,
 * and events don't reliably reach the webview on iOS.
 */
export default function useRemotePeers(): RemotePeer[] | null {
  const { data } = useQuery({
    queryKey: REMOTE_PEERS_QUERY_KEY,
    queryFn: () => createTauRPCProxy().cmd.list_remote_peers(),
    refetchInterval: 2000,
  });
  return data ?? null;
}
