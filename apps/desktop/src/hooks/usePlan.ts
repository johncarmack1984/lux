import { useQuery } from "@tanstack/react-query";
import {
  createTauRPCProxy,
  type PlanConnection,
  type PlanServiceType,
  type PlanView,
} from "@/bindings";

const plan = () => createTauRPCProxy().plan;

/** Query key for the Planning Center connection. */
export const PLAN_STATUS_QUERY_KEY = ["planStatus"] as const;
/** Query key for the connected church's service types. */
export const PLAN_SERVICE_TYPES_QUERY_KEY = ["planServiceTypes"] as const;
/** Query key for one service type's next plan. */
export const planQueryKey = (serviceTypeId: string | null) =>
  ["plan", serviceTypeId] as const;

/**
 * Whether a Planning Center organization is connected, and which. `null` until
 * the first read resolves.
 *
 * Refetches on window focus because the connection is completed *in a browser*,
 * outside the app: the church admin approves at Planning Center, closes the
 * tab, and comes back to lux. Focus is the moment there is something new to
 * learn, so it is the moment to ask.
 */
export default function usePlanStatus(): {
  status: PlanConnection | null;
  /** Set when the bridge could not be reached at all. */
  error: string | null;
} {
  const { data, error } = useQuery({
    queryKey: PLAN_STATUS_QUERY_KEY,
    queryFn: () => plan().plan_status(),
    refetchOnWindowFocus: true,
    // One retry, then say so. A view that spins forever because the bridge is
    // down is worse than one that admits it — the operator needs to know to
    // stop waiting and drive the desk by hand.
    retry: 1,
  });
  return {
    status: data ?? null,
    error: error ? String((error as Error).message ?? error) : null,
  };
}

/**
 * The connected church's service types. Only asked once connected.
 *
 * The error travels with them because the interesting failure here is a
 * *refusal*, not an outage: a revoked authorization answers "needs
 * reconnecting", and a caller that kept only the data would show an empty
 * calendar to a church whose connection simply needs renewing.
 */
export function usePlanServiceTypes(enabled: boolean): {
  serviceTypes: PlanServiceType[] | null;
  error: string | null;
} {
  const { data, error } = useQuery({
    queryKey: PLAN_SERVICE_TYPES_QUERY_KEY,
    queryFn: () => plan().plan_service_types(),
    enabled,
    // Service types change about once a year. Don't spend a church's rate
    // limit re-asking on every focus.
    staleTime: 5 * 60 * 1000,
    // One retry, then say so. "Needs reconnecting" and "not connected" are
    // settled answers, and asking three more times only delays the sentence
    // the operator has to act on.
    retry: 1,
  });
  return {
    serviceTypes: data ?? null,
    error: error ? String((error as Error).message ?? error) : null,
  };
}

/**
 * The next plan for a service type.
 *
 * Deliberately *not* polled. Reading a plan spends a church's Planning Center
 * rate limit, and a plan that changed on Saturday night is picked up by opening
 * the view or pressing refresh — the live-position polling that a service
 * actually needs is a different thing, on a different cadence, and it does not
 * exist yet.
 */
export function usePlan(serviceTypeId: string | null): {
  plan: PlanView | null;
  loading: boolean;
  error: string | null;
} {
  const { data, isFetching, error } = useQuery({
    queryKey: planQueryKey(serviceTypeId),
    queryFn: () => plan().plan_next(serviceTypeId ?? ""),
    enabled: Boolean(serviceTypeId),
    retry: false,
  });
  return {
    plan: data ?? null,
    loading: isFetching,
    error: error ? String((error as Error).message ?? error) : null,
  };
}
