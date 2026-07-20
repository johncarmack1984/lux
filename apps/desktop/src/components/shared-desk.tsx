import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { ArrowLeft, Link2, Loader2 } from "lucide-react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { createTauRPCProxy, type SharedDesk, type SharedSetup } from "@/bindings";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Slider } from "@/components/ui/slider";
import { cn, lightColorVariants } from "@/lib/utils";

const cmd = () => createTauRPCProxy().cmd;

/**
 * Roles cross the wire as plain strings so an owner on a newer app can add one
 * without breaking an older guest. Anything this build doesn't style renders as
 * a plain fader rather than an error.
 */
const STYLED_ROLES = [
  "Red",
  "Green",
  "Blue",
  "Amber",
  "White",
  "Brightness",
  "Generic",
] as const;
type StyledRole = (typeof STYLED_ROLES)[number];
const styledRole = (role: string): StyledRole =>
  (STYLED_ROLES as readonly string[]).includes(role)
    ? (role as StyledRole)
    : "Generic";

export const SHARED_SETUPS_QUERY_KEY = ["sharedSetups"];

/** Which desk is open, if any. */
type Open = { ownerSub: string; setupId: string };

/**
 * Setups other people have shared with this account, and the desk for one of
 * them.
 *
 * A guest holds no copy of anyone's setup: the channel list comes from the
 * owner's compiled config and the opening fader positions from their applier's
 * last-known buffer, both fetched fresh when a desk opens. Moving a fader here
 * publishes to the *owner's* rig and never touches this device's own fixtures.
 */
export default function SharedDeskView() {
  const queryClient = useQueryClient();
  const [open, setOpen] = useState<Open | null>(null);

  const { data: shared } = useQuery({
    queryKey: SHARED_SETUPS_QUERY_KEY,
    queryFn: () => cmd().list_shared_setups(),
    // Grants arrive by nudge-driven refresh on the backend; poll so the list
    // reflects them without needing an event to reach the webview (which iOS
    // drops).
    refetchInterval: 5000,
  });

  // Leaving the view is leaving the desk — the owner's surface should stop
  // showing this guest as live the moment they navigate away or quit.
  useEffect(() => () => void cmd().close_shared_desk(), []);

  if (open) {
    return (
      <Desk
        open={open}
        onBack={() => {
          void cmd().close_shared_desk();
          setOpen(null);
        }}
      />
    );
  }

  return (
    <div className="mx-auto flex h-full w-full max-w-3xl flex-col gap-4 px-4 py-4">
      <RedeemForm
        onClaimed={(claimed) => {
          void queryClient.invalidateQueries({ queryKey: SHARED_SETUPS_QUERY_KEY });
          toast.success(`${claimed.ownerLabel} shared a setup with you`);
        }}
      />

      {shared?.length ? (
        <ul className="flex flex-col gap-2">
          {shared.map((s) => (
            <li key={`${s.ownerSub}:${s.setupId}`}>
              <button
                type="button"
                onClick={() => setOpen({ ownerSub: s.ownerSub, setupId: s.setupId })}
                className="flex w-full items-center justify-between rounded-md border px-3 py-2 text-left hover:bg-accent"
              >
                <span className="flex flex-col">
                  <span className="text-sm font-medium">
                    {s.setupName ?? "Shared setup"}
                  </span>
                  <span className="text-xs text-muted-foreground">
                    {s.ownerLabel}
                  </span>
                </span>
                {/* A grant without a config means the owner's app isn't
                    running — there is nothing to draw, and saying so beats
                    opening an empty desk. */}
                <span className="text-xs text-muted-foreground">
                  {s.renderable ? "Open" : "Offline"}
                </span>
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="text-sm text-muted-foreground">
          Nothing has been shared with you yet. When someone sends you an invite
          code, enter it above.
        </p>
      )}
    </div>
  );
}

/** Redeem an invite code someone sent over their own channel. */
function RedeemForm({ onClaimed }: { onClaimed: (s: SharedSetup) => void }) {
  const [code, setCode] = useState("");
  const [pending, setPending] = useState(false);

  const claim = async () => {
    if (!code.trim() || pending) return;
    setPending(true);
    try {
      onClaimed(await cmd().claim_share(code.trim()));
      setCode("");
    } catch (e) {
      // The server says the same thing for unknown, expired, and already-used
      // codes, so this is the whole story the UI has — and all it should tell.
      toast.error(String(e));
    } finally {
      setPending(false);
    }
  };

  return (
    <div className="flex items-center gap-2">
      <Link2 className="size-4 shrink-0 text-muted-foreground" />
      <Input
        value={code}
        onChange={(e) => setCode(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") void claim();
        }}
        // Case, dashes, and stray spaces are all normalized server-side, so
        // whatever the messaging app did to the code is fine.
        placeholder="Invite code (LUX-XXXXX-XXXXX)"
        className="h-9"
        autoCapitalize="characters"
        autoCorrect="off"
        spellCheck={false}
      />
      <Button size="sm" onClick={claim} disabled={!code.trim() || pending}>
        {pending ? <Loader2 className="size-4 animate-spin" /> : "Redeem"}
      </Button>
    </div>
  );
}

/**
 * The desk for one shared setup. Fader positions are local UI state seeded from
 * the owner's last-known buffer — there is no local buffer behind this view,
 * and every move goes straight out to the owner.
 */
function Desk({ open, onBack }: { open: Open; onBack: () => void }) {
  const [values, setValues] = useState<Record<number, number>>({});
  const seeded = useRef(false);

  const { data: desk, isPending } = useQuery({
    queryKey: ["sharedDesk", open.ownerSub, open.setupId],
    queryFn: () => cmd().open_shared_desk(open.ownerSub, open.setupId),
  });

  useEffect(() => {
    if (!desk || seeded.current) return;
    seeded.current = true;
    const seed: Record<number, number> = {};
    for (const ch of desk.channels) seed[ch.n] = desk.buffer[ch.n - 1] ?? 0;
    setValues(seed);
  }, [desk]);

  const drag = (n: number, next: number) => {
    setValues((v) => ({ ...v, [n]: next }));
    // Coalesced to ~25 Hz on the backend, the same as a local drag.
    void cmd().set_shared_channel(n, next);
  };

  const header = (title: string, subtitle?: string) => (
    <div className="flex items-center gap-2">
      <Button variant="ghost" size="sm" onClick={onBack}>
        <ArrowLeft className="size-4" />
      </Button>
      <span className="flex flex-col">
        <span className="text-sm font-medium">{title}</span>
        {subtitle ? (
          <span className="text-xs text-muted-foreground">{subtitle}</span>
        ) : null}
      </span>
    </div>
  );

  if (isPending) {
    return <div className="px-4 py-4">{header("Opening…")}</div>;
  }

  if (!desk) {
    return (
      <div className="mx-auto flex w-full max-w-3xl flex-col gap-3 px-4 py-4">
        {header("Not available")}
        <p className="text-sm text-muted-foreground">
          The owner&rsquo;s app isn&rsquo;t running, so there&rsquo;s nothing to
          control yet. This page will work as soon as it is.
        </p>
      </div>
    );
  }

  const unpatched = desk.channels.length === 0;

  return (
    <div className="mx-auto flex h-full w-full max-w-3xl flex-col gap-3 px-4 py-4">
      {header(desk.name, `Universe ${desk.universe}`)}
      {unpatched ? (
        <p className="text-sm text-muted-foreground">
          This setup has no fixtures patched, so there are no named controls to
          show.
        </p>
      ) : (
        <ul className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto">
          {desk.channels.map((ch) => (
            <li key={ch.n} className="flex items-center gap-3">
              <span
                className={cn(
                  "shrink-0 text-xs tabular-nums",
                  // An unfamiliar role falls through to the neutral style
                  // rather than breaking the row.
                  lightColorVariants({ labelColor: styledRole(ch.role) }),
                )}
              >
                {ch.n}
              </span>
              <span className="w-24 shrink-0 truncate text-right text-sm text-muted-foreground">
                {ch.name}
              </span>
              <span className="w-10 shrink-0 text-xs tabular-nums text-muted-foreground">
                {(values[ch.n] ?? 0).toString().padStart(3, "0")}
              </span>
              <Slider
                aria-label={`${ch.name} (channel ${ch.n})`}
                value={[values[ch.n] ?? 0]}
                onValueChange={([next]) => drag(ch.n, next)}
                max={255}
                step={1}
                className="flex-1"
              />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export type { SharedDesk };
