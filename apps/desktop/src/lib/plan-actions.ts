import { createTauRPCProxy, type PlanConnection } from "@/bindings";
import { queryClient } from "@/lib/query-client";
import {
  PLAN_SERVICE_TYPES_QUERY_KEY,
  PLAN_STATUS_QUERY_KEY,
} from "@/hooks/usePlan";

/**
 * The Tauri adapter for the Planning Center bridge.
 *
 * Nothing here touches the lights. Connecting, disconnecting, and reading a
 * plan are all account-level operations — the rig keeps doing exactly what it
 * was doing, which is the rule the whole bridge is built on.
 */
const plan = () => createTauRPCProxy().plan;

/**
 * Start a connection. The backend opens the church admin's default browser at
 * Planning Center's consent screen and hands back the same URL, so the surface
 * can offer it as a link if no browser opened.
 */
export async function connectPlanningCenter(): Promise<string> {
  const { authorizeUrl } = await plan().plan_connect();
  return authorizeUrl;
}

/** Forget the church's Planning Center tokens. */
export async function disconnectPlanningCenter(): Promise<PlanConnection> {
  const status = await plan().plan_disconnect();
  queryClient.setQueryData(PLAN_STATUS_QUERY_KEY, status);
  // The old church's service types are now somebody else's business.
  queryClient.removeQueries({ queryKey: PLAN_SERVICE_TYPES_QUERY_KEY });
  queryClient.removeQueries({ queryKey: ["plan"] });
  return status;
}

/**
 * Re-read the connection — what the surface calls after the admin says they
 * finished at Planning Center, and on window focus.
 */
export async function refreshPlanStatus(): Promise<void> {
  await queryClient.invalidateQueries({ queryKey: PLAN_STATUS_QUERY_KEY });
}

/**
 * Re-read this week's plan.
 *
 * Separate from {@link refreshPlanStatus} and the thing the Refresh button
 * actually wants: the plan is what changed on Saturday night, and re-asking
 * only about the connection would leave the operator looking at a stale list
 * that the button just told them was fresh.
 */
export async function refreshPlan(): Promise<void> {
  await queryClient.invalidateQueries({ queryKey: ["plan"] });
}
