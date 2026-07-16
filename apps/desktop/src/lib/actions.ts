import { createTauRPCProxy, type SliderOrientation } from "@/bindings";
import { queryClient } from "@/lib/query-client";
import { BUFFER_QUERY_KEY } from "@/hooks/useBuffer";
import { SETTINGS_QUERY_KEY } from "@/hooks/useSettings";

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

/**
 * Flip the desk's fader orientation. Same cache-through pattern as
 * {@link setChannelValue}: the returned settings land in the query cache
 * directly, so the grid re-lays-out without waiting on the `settingsChanged`
 * event (undelivered to the webview on iOS).
 */
async function setSliderOrientation(orientation: SliderOrientation) {
  const taurpc = createTauRPCProxy();
  const settings = await taurpc.cmd.set_slider_orientation(orientation);
  // Kill any in-flight settings refetch first: one that started before the
  // backend committed would resolve after this write and revert the cache.
  await queryClient.cancelQueries({ queryKey: SETTINGS_QUERY_KEY });
  queryClient.setQueryData(SETTINGS_QUERY_KEY, settings);
  return settings;
}

export { setChannelValue, setSliderOrientation };
