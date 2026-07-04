import type { LuxChannel } from "@/global";
import { useQuery } from "@tanstack/react-query";
import { createTauRPCProxy } from "@/bindings";

/** Query key for the channel metadata (label + color per channel). */
export const CHANNELS_QUERY_KEY = ["channels"] as const;

/**
 * The 512 channels' metadata (label + color). `null` until the first read.
 *
 * This changes only when fixtures are patched — which the backend echoes on the
 * `channelDataSet` event, undelivered to the webview on iOS. So it refreshes
 * with the fixture views (see useLuxRefresh), plus refetch-on-focus for
 * cloud-synced changes, rather than relying on the event.
 */
function useChannelData(): LuxChannel[] | null {
  const { data } = useQuery({
    queryKey: CHANNELS_QUERY_KEY,
    queryFn: () =>
      createTauRPCProxy()
        .sync.sync_channels()
        .then((c) => c.channels),
  });
  return data ?? null;
}

export default useChannelData;
