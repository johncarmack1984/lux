import { useQueryClient } from "@tanstack/react-query";
import { FIXTURES_QUERY_KEY } from "./useFixtures";
import { SETUPS_QUERY_KEY } from "./useSetups";
import { CHANNELS_QUERY_KEY } from "./useChannelsMetaData";
import { BUFFER_QUERY_KEY } from "./useBuffer";

/**
 * Refetch the reactive views a fixture/setup mutation can affect. The backend's
 * `patchSet` / `setupsChanged` / `channelDataSet` / `bufferSet` events aren't
 * reliably delivered to the webview on iOS, so a mutation refreshes
 * authoritatively instead of relying on them. All four keys refresh together
 * because they're coupled — switching a setup swaps the active patch, its
 * channel labels, and the live buffer; patching a fixture changes the fixture
 * count and the channel metadata.
 */
export default function useLuxRefresh() {
  const queryClient = useQueryClient();
  return () =>
    Promise.all(
      [
        FIXTURES_QUERY_KEY,
        SETUPS_QUERY_KEY,
        CHANNELS_QUERY_KEY,
        BUFFER_QUERY_KEY,
      ].map((queryKey) => queryClient.invalidateQueries({ queryKey }))
    );
}
