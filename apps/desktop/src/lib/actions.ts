import { createTauRPCProxy } from "@/bindings";
import { queryClient } from "@/lib/query-client";
import { BUFFER_QUERY_KEY } from "@/hooks/useBuffer";

/**
 * Set one channel's value. Pushes the buffer the backend returns straight into
 * the query cache, so every buffer view reflects the change without waiting on
 * the `bufferSet` event (undelivered to the webview on iOS).
 */
async function setChannelValue({
  channelNumber,
  value,
}: {
  channelNumber: number;
  value: number;
}) {
  const taurpc = createTauRPCProxy();
  const buffer = await taurpc.cmd.update_channel_value(channelNumber, value);
  queryClient.setQueryData(BUFFER_QUERY_KEY, buffer.buffer);
  return buffer;
}

export { setChannelValue };
