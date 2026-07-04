import { useQuery } from "@tanstack/react-query";
import { createTauRPCProxy, type AuthStatus } from "@/bindings";

/** Query key for the account status; auth actions invalidate this to refetch. */
export const AUTH_QUERY_KEY = ["auth"] as const;

/**
 * The current account status: whether accounts are configured, whether someone
 * is signed in, and their email. `null` until the first read resolves.
 *
 * The backend emits an `authChanged` event, but it is not reliably delivered to
 * the webview on iOS, so this does not depend on it. Instead: `auth_status`
 * reads the in-memory session (a cheap, no-network command), the account UI
 * invalidates [`AUTH_QUERY_KEY`] after every sign-in / sign-out / delete for an
 * immediate authoritative update, the query refetches on window focus, and it
 * polls a few times just after mount to catch a session restored asynchronously
 * from the keychain at startup (then stops).
 */
export default function useAuth(): AuthStatus | null {
  const { data } = useQuery({
    queryKey: AUTH_QUERY_KEY,
    queryFn: () => createTauRPCProxy().cmd.auth_status(),
    refetchOnWindowFocus: true,
    refetchInterval: (query) => {
      if (query.state.data?.signedIn) return false; // settled — stop polling
      return query.state.dataUpdateCount >= 4 ? false : 1500; // ~4 tries for startup restore
    },
  });
  return data ?? null;
}
