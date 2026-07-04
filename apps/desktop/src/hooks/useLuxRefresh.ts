import { useQueryClient } from "@tanstack/react-query";
import { FIXTURES_QUERY_KEY } from "./useFixtures";
import { SETUPS_QUERY_KEY } from "./useSetups";

/**
 * Refetch the fixture + setup views after a mutation. The backend's `patchSet`
 * and `setupsChanged` events aren't reliably delivered to the webview on iOS,
 * so a mutation refreshes authoritatively instead of relying on them. Both keys
 * refresh together because they're coupled — switching a setup swaps the
 * active patch, and adding a fixture changes the active setup's fixture count.
 */
export default function useLuxRefresh() {
  const queryClient = useQueryClient();
  return () =>
    Promise.all([
      queryClient.invalidateQueries({ queryKey: FIXTURES_QUERY_KEY }),
      queryClient.invalidateQueries({ queryKey: SETUPS_QUERY_KEY }),
    ]);
}
