import { useQuery } from "@tanstack/react-query";
import { createTauRPCProxy, type Fixture } from "@/bindings";

/** Query key for the active setup's patched fixtures. */
export const FIXTURES_QUERY_KEY = ["fixtures"] as const;

/**
 * The active setup's patched fixtures. `null` while the first read is in flight.
 *
 * The backend emits a `patchSet` event on every add/remove/update, but it is
 * not reliably delivered to the webview on iOS — so this reads through TanStack
 * Query and stays current via `useLuxRefresh` (mutations invalidate this key)
 * plus refetch-on-focus (which picks up cloud-synced changes) rather than the
 * event.
 */
export default function useFixtures(): Fixture[] | null {
  const { data } = useQuery({
    queryKey: FIXTURES_QUERY_KEY,
    queryFn: () => createTauRPCProxy().cmd.get_patch(),
  });
  return data ?? null;
}
