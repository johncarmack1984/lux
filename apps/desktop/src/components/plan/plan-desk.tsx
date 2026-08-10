import { useEffect, useState } from "react";
import { toast } from "sonner";
import {
  CalendarDays,
  ExternalLink,
  Loader2,
  Play,
  RefreshCw,
  Unplug,
} from "lucide-react";
import { type PlanItemRow, type Scene } from "@/bindings";
import usePlanStatus, { usePlan, usePlanServiceTypes } from "@/hooks/usePlan";
import useScenes from "@/hooks/useScenes";
import useBuffer from "@/hooks/useBuffer";
import { activeSceneId } from "@/lib/scene-state";
import { recallScene } from "@/lib/scene-actions";
import {
  connectPlanningCenter,
  disconnectPlanningCenter,
  refreshPlan,
  refreshPlanStatus,
} from "@/lib/plan-actions";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

/** "4:30" for a plan item's planned length. */
function length(seconds: number | null): string | null {
  if (seconds === null || seconds <= 0) return null;
  const total = Math.round(seconds);
  const minutes = Math.floor(total / 60);
  return `${minutes}:${String(total % 60).padStart(2, "0")}`;
}

/**
 * Planning Center's own item types, softened into words a volunteer reads.
 * Anything a church invented for itself falls through as its own label rather
 * than an error — the same reader-tolerates-what-it-doesn't-know rule the cue
 * map's `itemType` follows.
 */
const ITEM_TYPES: Record<string, string> = {
  song: "Song",
  header: "Header",
  media: "Media",
  item: "Item",
};

function PlanRow({
  item,
  scenes,
  showing,
  disabled,
}: {
  item: PlanItemRow;
  scenes: Scene[] | null;
  showing: string | null;
  disabled: boolean;
}) {
  // Which scene this row fires, chosen here and held only in this view. When a
  // cue map lands, `item.sceneId` becomes the answer and this local pick
  // becomes the override — the row does not change shape either way.
  const [picked, setPicked] = useState<string | null>(item.sceneId);
  useEffect(() => setPicked(item.sceneId), [item.sceneId]);

  const isHeader = item.itemType === "header";
  const runtime = length(item.lengthS);

  return (
    <li
      className={cn(
        "flex flex-wrap items-center gap-x-3 gap-y-2 rounded-lg border px-3 py-2",
        isHeader && "bg-muted/40 font-medium"
      )}
    >
      <div className="min-w-0 flex-1">
        <p className="truncate text-sm">{item.title || "Untitled"}</p>
        <p className="text-xs text-muted-foreground">
          {ITEM_TYPES[item.itemType] ?? item.itemType}
          {runtime ? ` · ${runtime}` : ""}
        </p>
      </div>

      {scenes && scenes.length > 0 && (
        <>
          <select
            aria-label={`Scene for ${item.title || "this item"}`}
            className="h-8 rounded-md border bg-background px-2 text-sm"
            value={picked ?? ""}
            onChange={(e) => setPicked(e.target.value || null)}
          >
            <option value="">— no scene —</option>
            {scenes.map((scene) => (
              <option key={scene.id} value={scene.id}>
                {scene.name}
              </option>
            ))}
          </select>
          <Button
            size="sm"
            className="gap-1.5"
            disabled={disabled || !picked}
            aria-label={`Go — ${item.title || "this item"}`}
            onClick={() => {
              if (!picked) return;
              recallScene(picked).catch((e: unknown) => toast.error(String(e)));
            }}
          >
            <Play className="size-3.5" />
            Go
          </Button>
          {picked && picked === showing && (
            <span className="text-xs text-muted-foreground">on</span>
          )}
        </>
      )}
    </li>
  );
}

/**
 * The service plan, as a cue list.
 *
 * This is the read-and-drive-by-hand half of the bridge: the plan comes from
 * Planning Center, the scenes are lux's own, and the operator presses Go. The
 * cue map that makes next week free — and the live following that presses Go
 * for them — are the next unit, and this view is the surface both land on.
 *
 * Nothing here can black out a rig. Every Go is an ordinary scene recall
 * through the same crossfade a hand-pressed one uses, and losing Planning
 * Center costs the plan list, never the lights.
 */
export default function PlanDesk() {
  const { status, error: statusError } = usePlanStatus();
  const scenes = useScenes();
  const buffer = useBuffer();
  const showing = activeSceneId(buffer, scenes);

  const connected = status?.connected ?? false;
  const serviceTypes = usePlanServiceTypes(connected);
  const [serviceTypeId, setServiceTypeId] = useState<string | null>(null);

  // Land on the church's first service type. Most have exactly one, and making
  // them pick it before they can see anything is a step for nothing.
  useEffect(() => {
    if (serviceTypeId === null && serviceTypes?.length) {
      setServiceTypeId(serviceTypes[0].id);
    }
  }, [serviceTypes, serviceTypeId]);

  const { plan, loading, error } = usePlan(connected ? serviceTypeId : null);
  const [connecting, setConnecting] = useState(false);
  const [consentUrl, setConsentUrl] = useState<string | null>(null);

  // The connection finishes in a browser, outside this window. Coming back to
  // lux is the signal to re-ask.
  useEffect(() => {
    const onFocus = () => void refreshPlanStatus();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, []);

  // The bridge is unreachable. Say so rather than spinning: the desk still
  // works, and the operator needs to know to drive it by hand.
  if (statusError) {
    return (
      <Empty
        title="Can’t reach the plan service"
        detail={`${statusError} — your lights and scenes are unaffected. Drive the desk from Fixtures or Universe in the meantime.`}
      />
    );
  }

  if (status === null) {
    return (
      <div className="flex flex-1 items-center justify-center text-sm text-muted-foreground">
        <Loader2 className="mr-2 size-4 animate-spin" />
        Checking Planning Center…
      </div>
    );
  }

  // Nothing to connect *to*: either this build predates the bridge or nobody
  // is signed in. Say the true thing rather than offering a button that fails.
  if (!status.available) {
    return (
      <Empty
        title="Planning Center isn’t available on this build"
        detail="Sign in to your lux account to connect a Planning Center organization. The desk works exactly as it does now either way."
      />
    );
  }

  if (!connected) {
    return (
      <div className="mx-auto flex w-full max-w-2xl flex-col gap-4 px-4 py-8">
        <div className="rounded-xl border p-5">
          <h2 className="text-base font-medium">Follow your service plan</h2>
          <p className="mt-1 text-sm text-muted-foreground">
            Connect your church’s Planning Center account and this week’s plan
            becomes your cue list — every week, without rebuilding it.
          </p>
          <p className="mt-3 text-sm text-muted-foreground">
            lux only ever <span className="font-medium">reads</span> your plans.
            It never changes a plan and never advances your service.
          </p>
          <div className="mt-4 flex flex-wrap items-center gap-3">
            <Button
              disabled={connecting}
              onClick={() => {
                setConnecting(true);
                connectPlanningCenter()
                  .then((url) => setConsentUrl(url))
                  .catch((e: unknown) => toast.error(String(e)))
                  .finally(() => setConnecting(false));
              }}
            >
              {connecting ? (
                <Loader2 className="mr-2 size-4 animate-spin" />
              ) : (
                <ExternalLink className="mr-2 size-4" />
              )}
              Connect Planning Center
            </Button>
            <Button variant="ghost" onClick={() => void refreshPlanStatus()}>
              I’ve finished — check again
            </Button>
          </div>
          {consentUrl && (
            <p className="mt-4 break-all rounded-lg bg-muted/50 p-3 text-xs text-muted-foreground">
              Didn’t open? Paste this into a browser:
              <br />
              {consentUrl}
            </p>
          )}
          <p className="mt-4 text-xs text-muted-foreground">
            You’ll need to be an Administrator on your Planning Center
            organization to approve this.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="mx-auto flex h-full w-full max-w-3xl flex-col gap-3 px-4 py-4">
      <div className="flex flex-wrap items-center gap-3">
        <CalendarDays className="size-4 text-muted-foreground" />
        <div className="min-w-0">
          <p className="truncate text-sm">
            {status.orgName ?? "Planning Center"}
          </p>
          {status.needsReconnect && (
            <p className="text-xs text-amber-600 dark:text-amber-500">
              This connection expires soon — reconnect before Sunday.
            </p>
          )}
        </div>

        {serviceTypes && serviceTypes.length > 1 && (
          <select
            aria-label="Service type"
            className="h-8 rounded-md border bg-background px-2 text-sm"
            value={serviceTypeId ?? ""}
            onChange={(e) => setServiceTypeId(e.target.value || null)}
          >
            {serviceTypes.map((type) => (
              <option key={type.id} value={type.id}>
                {type.name}
              </option>
            ))}
          </select>
        )}

        <div className="ml-auto flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            className="gap-1.5"
            disabled={loading || !serviceTypeId}
            onClick={() => void refreshPlan()}
          >
            <RefreshCw className={cn("size-3.5", loading && "animate-spin")} />
            Refresh
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className="gap-1.5"
            onClick={() => {
              disconnectPlanningCenter()
                .then(() => {
                  // The next church's service types are not this one's.
                  setServiceTypeId(null);
                  toast.success("Planning Center disconnected");
                })
                .catch((e: unknown) => toast.error(String(e)));
            }}
          >
            <Unplug className="size-3.5" />
            Disconnect
          </Button>
        </div>
      </div>

      {error && (
        <p className="rounded-lg border border-destructive/40 bg-destructive/5 px-3 py-2 text-sm">
          {error}
        </p>
      )}

      {plan?.planId ? (
        <>
          <div>
            <p className="text-sm font-medium">{plan.dates ?? "Next plan"}</p>
            {plan.title && (
              <p className="text-xs text-muted-foreground">{plan.title}</p>
            )}
          </div>
          <ul className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto pb-2">
            {plan.items.map((item) => (
              <PlanRow
                key={item.id}
                item={item}
                scenes={scenes}
                showing={showing}
                disabled={buffer === null}
              />
            ))}
          </ul>
          {scenes !== null && scenes.length === 0 && (
            <p className="rounded-lg border border-dashed px-3 py-2 text-center text-xs text-muted-foreground">
              Save a look as a scene and it becomes available on every row here.
            </p>
          )}
        </>
      ) : loading ? (
        <div className="flex flex-1 items-center justify-center text-sm text-muted-foreground">
          <Loader2 className="mr-2 size-4 animate-spin" />
          Reading the plan…
        </div>
      ) : (
        <Empty
          title="No upcoming plan"
          detail="There’s nothing on this service type’s calendar yet. It’ll show up here as soon as it’s scheduled in Planning Center."
        />
      )}
    </div>
  );
}

function Empty({ title, detail }: { title: string; detail: string }) {
  return (
    <div className="mx-auto flex w-full max-w-lg flex-1 flex-col items-center justify-center gap-2 px-6 text-center">
      <p className="text-sm font-medium">{title}</p>
      <p className="text-sm text-muted-foreground">{detail}</p>
    </div>
  );
}
