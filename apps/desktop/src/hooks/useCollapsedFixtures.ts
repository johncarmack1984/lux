import { useQuery } from "@tanstack/react-query";
import { createTauRPCProxy } from "@/bindings";

/** Query key for the set of collapsed fixture cards (device-local UI state). */
export const COLLAPSED_FIXTURES_QUERY_KEY = ["collapsedFixtures"] as const;

/**
 * Which fixture cards are collapsed, as a Set of fixture ids. `null` until the
 * first read resolves — gate rendering on it, like the settings read, so a
 * collapsed card doesn't flash open on launch. The setter in `lib/actions`
 * writes the backend's returned set straight into the cache.
 */
export default function useCollapsedFixtures(): Set<string> | null {
  const { data } = useQuery({
    queryKey: COLLAPSED_FIXTURES_QUERY_KEY,
    queryFn: () => createTauRPCProxy().cmd.get_collapsed_fixtures(),
  });
  return data ? new Set(data) : null;
}
