import { useQuery } from "@tanstack/react-query";
import { createTauRPCProxy, type SetupSummary } from "@/bindings";

/** Query key for the user's setups and which one is active. */
export const SETUPS_QUERY_KEY = ["setups"] as const;

/**
 * The user's setups (which one is active is carried on each summary). `null`
 * while the first read is in flight.
 *
 * The backend emits a `setupsChanged` event on create/rename/delete/switch, but
 * it is not reliably delivered to the webview on iOS — so this reads through
 * TanStack Query and stays current via `useLuxRefresh` (mutations invalidate
 * this key) plus refetch-on-focus rather than the event.
 */
export default function useSetups(): SetupSummary[] | null {
  const { data } = useQuery({
    queryKey: SETUPS_QUERY_KEY,
    queryFn: () => createTauRPCProxy().cmd.list_setups(),
  });
  return data ?? null;
}
