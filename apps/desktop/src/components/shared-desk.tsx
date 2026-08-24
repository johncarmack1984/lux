import { useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import { ArrowLeft, Link2, Loader2 } from "lucide-react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  createTauRPCProxy,
  type Fixture,
  type LuxLabelColor,
  type SharedDesk,
  type SharedSetup,
} from "@/bindings";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import FixtureCard from "@/components/fixtures/fixture-card";
import { PresetRow } from "@/components/button-row";
import useSettings from "@/hooks/useSettings";
import { DeskProvider, type Desk as DeskSeam } from "@/lib/desk-context";
import { createPresetStore } from "@/lib/preset-toggle";
import { cn } from "@/lib/utils";

const cmd = () => createTauRPCProxy().cmd;

/**
 * Roles cross the wire as plain strings so an owner on a newer app can add one
 * without breaking an older guest. Anything this build doesn't recognize
 * renders as a Generic fader rather than an error.
 */
const STYLED_ROLES: readonly LuxLabelColor[] = [
  "Red",
  "Green",
  "Blue",
  "Amber",
  "White",
  "Brightness",
  "Generic",
];
const styledRole = (role: string): LuxLabelColor =>
  (STYLED_ROLES as readonly string[]).includes(role)
    ? (role as LuxLabelColor)
    : "Generic";

export const SHARED_SETUPS_QUERY_KEY = ["sharedSetups"];

/** Which desk is open, if any. */
type Open = { ownerSub: string; setupId: string };

/**
 * Setups other people have shared with this account, and the desk for one of
 * them.
 *
 * A guest holds no copy of anyone's setup: the fixture list comes from the
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
 * How long a just-written channel ignores the polled state echo. The echo of a
 * guest's own write takes a full round trip to come back (guest coalesce →
 * owner apply → echo coalesce → retained delivery → next poll), so following
 * it immediately would snap a fader backwards mid-drag or drop a just-engaged
 * preset. Anything older than this is an out-of-band change worth showing.
 */
const WRITE_HOLDOFF_MS = 800;

const UNIVERSE_SIZE = 512;

/**
 * The desk for one shared setup — the owner's fixtures view, pointed at their
 * rig. The compiled config's fixture list is regrouped into the same shape the
 * owner's cards render (name, span, role-labelled channels), and every control
 * writes through a guest desk seam instead of the local buffer. The patch is
 * the owner's, so the cards are read-only; the levels are live both ways.
 */
function Desk({ open, onBack }: { open: Open; onBack: () => void }) {
  // The guest's local picture of the owner's buffer: seeded from the applier's
  // last-known frame, advanced optimistically by this desk's own writes, and
  // folded toward the polled state echo for everything not just written here.
  const [buffer, setBuffer] = useState<number[] | null>(null);
  const bufferRef = useRef<number[] | null>(null);
  bufferRef.current = buffer;
  const heldAt = useRef<Map<number, number>>(new Map());
  // Collapse state is per-session here — a guest has no setups.json row for
  // someone else's fixtures, and remembering less about another account's rig
  // is a feature.
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());

  const settings = useSettings();
  const vertical =
    settings !== null && (settings.sliderOrientation ?? "vertical") === "vertical";

  const { data: desk, isPending } = useQuery({
    queryKey: ["sharedDesk", open.ownerSub, open.setupId],
    queryFn: () => cmd().open_shared_desk(open.ownerSub, open.setupId),
  });

  useEffect(() => {
    if (!desk || bufferRef.current) return;
    const seed = new Array<number>(UNIVERSE_SIZE).fill(0);
    desk.buffer.forEach((v, i) => {
      seed[i] = v;
    });
    setBuffer(seed);
  }, [desk]);

  // Follow the owner's rig live: poll the applier's state echo (a cheap
  // in-memory read; polling because webview events never reach iOS, the same
  // trade as useBuffer) and fold it into every channel not just written here —
  // a scene fade or another surface's move shows as it happens.
  const { data: live } = useQuery({
    queryKey: ["sharedDeskBuffer", open.ownerSub, open.setupId],
    queryFn: () => cmd().shared_desk_buffer(open.ownerSub, open.setupId),
    refetchInterval: 200,
    enabled: !!desk,
  });

  useEffect(() => {
    if (!live?.length) return;
    const now = Date.now();
    setBuffer((b) => {
      if (!b) return b;
      let changed = false;
      const next = [...b];
      for (let i = 0; i < next.length; i++) {
        const held = heldAt.current.get(i + 1);
        if (held && now - held < WRITE_HOLDOFF_MS) continue;
        const echoed = live[i] ?? 0;
        if (next[i] !== echoed) {
          next[i] = echoed;
          changed = true;
        }
      }
      return changed ? next : b;
    });
  }, [live]);

  // This desk's preset lane: plans against the guest's local picture of the
  // buffer (the freshest truth a guest has) and writes full frames to the
  // owner's rig. A store per open desk, so nothing leaks between visits or
  // into the owner-side singleton.
  const seam = useMemo<DeskSeam>(() => {
    const localWrite = (writes: ReadonlyArray<[number, number]>) => {
      const now = Date.now();
      setBuffer((b) => {
        const next = b ? [...b] : new Array<number>(UNIVERSE_SIZE).fill(0);
        for (const [n, value] of writes) {
          heldAt.current.set(n, now);
          next[n - 1] = value;
        }
        return next;
      });
    };
    return {
      editable: false,
      async setChannel(channelNumber, value) {
        localWrite([[channelNumber, value]]);
        // Coalesced to ~25 Hz on the backend, the same as a local drag.
        await cmd().set_shared_channel(channelNumber, value);
      },
      async setCollapsed(fixtureId, next) {
        setCollapsed((prev) => {
          const ids = new Set(prev);
          if (next) ids.add(fixtureId);
          else ids.delete(fixtureId);
          return ids;
        });
      },
      presets: createPresetStore({
        cached: () => bufferRef.current ?? undefined,
        read: async () =>
          bufferRef.current ??
          (await cmd().shared_desk_buffer(open.ownerSub, open.setupId)),
        async apply(frame) {
          await cmd().set_shared_buffer(frame);
          localWrite(frame.map((value, i) => [i + 1, value]));
        },
      }),
    };
  }, [open.ownerSub, open.setupId]);

  // Regroup the compiled config into the owner's card shape. The config's
  // flat channel list covers each fixture's span densely (it is compiled from
  // the same fixtures), so slicing on [address, address + count) recovers the
  // per-fixture channel order the color mixer needs. Fixture ids don't cross
  // the wire; the address is unique within a patch and stands in.
  const fixtures = useMemo<Fixture[]>(
    () =>
      (desk?.fixtures ?? []).map((f) => ({
        id: `shared-${f.address}`,
        name: f.name,
        address: f.address,
        channels: desk!.channels
          .filter((c) => c.n >= f.address && c.n < f.address + f.count)
          .sort((a, b) => a.n - b.n)
          .map((c) => ({ role: styledRole(c.role), label: c.name })),
      })),
    [desk],
  );

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

  return (
    <DeskProvider value={seam}>
      <div className="mx-auto flex h-full w-full max-w-3xl flex-col gap-3 overflow-y-auto px-4 py-4">
        {header(desk.name, `Universe ${desk.universe}`)}
        <PresetRow buffer={buffer} />
        {desk.scenes.length > 0 ? (
          <div className="flex flex-wrap gap-2">
            {desk.scenes.map((scene) => (
              <Button
                key={scene.id}
                variant="outline"
                size="sm"
                // The owner's applier resolves the id and runs the fade; the
                // desk follows it through the polled state echo above.
                onClick={() => void cmd().recall_shared_scene(scene.id)}
              >
                {scene.name}
              </Button>
            ))}
          </div>
        ) : null}
        {fixtures.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            This setup has no fixtures patched, so there is nothing to show.
          </p>
        ) : (
          // The owner's fixtures layout: cards sorted by address, side by side
          // as a console when the guest's own fader orientation is vertical.
          <div
            className={cn(
              "flex w-full flex-col gap-4",
              vertical && "min-h-0 flex-1 flex-row gap-4 overflow-x-auto pb-2",
            )}
          >
            {[...fixtures]
              .sort((a, b) => a.address - b.address)
              .map((fixture) => (
                <FixtureCard
                  key={fixture.id}
                  fixture={fixture}
                  buffer={buffer}
                  vertical={vertical}
                  collapsed={collapsed.has(fixture.id)}
                />
              ))}
          </div>
        )}
      </div>
    </DeskProvider>
  );
}

export type { SharedDesk };
