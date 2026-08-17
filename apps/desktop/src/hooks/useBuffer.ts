import { useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { createTauRPCProxy } from "@/bindings";
import useRemotePeers from "./useRemotePeers";

/** Query key for the live DMX buffer (the current value of every channel). */
export const BUFFER_QUERY_KEY = ["buffer"] as const;

/**
 * The live DMX buffer — the current value of every channel. `null` until the
 * first read resolves.
 *
 * Faders keep their own optimistic state so dragging is always smooth; this
 * query reflects changes made elsewhere. The channel-value setter pushes the
 * buffer it gets back straight into the cache (see lib/actions), which is what
 * keeps it current on iOS. On desktop the `bufferSet` event is also honored as
 * a fast path so out-of-band changes — a remote command over IoT — still show
 * live; the poll below covers iOS where that event never reaches the webview,
 * but only runs when another device or guest is connected.
 */
export default function useBuffer(): number[] | null {
  const queryClient = useQueryClient();
  const peers = useRemotePeers();
  const hasPeers = !!peers?.length;

  const { data } = useQuery({
    queryKey: BUFFER_QUERY_KEY,
    queryFn: () =>
      createTauRPCProxy()
        .sync.sync_buffer()
        .then((b) => b.buffer),
    refetchInterval: hasPeers ? 200 : false,
  });

  useEffect(() => {
    const unlisten = createTauRPCProxy().sync.event.on((event) => {
      if (event.type === "bufferSet") {
        queryClient.setQueryData(BUFFER_QUERY_KEY, event.buffer);
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [queryClient]);

  return data ?? null;
}
