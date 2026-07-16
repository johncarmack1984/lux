import { useEffect, useState } from "react";
import { toast } from "sonner";
import {
  ChevronsDownUp,
  ChevronsLeftRight,
  ChevronsRightLeft,
  ChevronsUpDown,
  Trash2,
} from "lucide-react";
import { createTauRPCProxy, type Fixture } from "@/bindings";
import useLuxRefresh from "@/hooks/useLuxRefresh";
import { setFixtureCollapsed } from "@/lib/actions";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import FixtureChannel from "./fixture-channel";
import FixtureColor from "./fixture-color";

const COLOR_ROLES = ["Red", "Green", "Blue"] as const;

/** "Living Room" → "LR", "Fixture 12" → "F12": word initials, whole numbers. */
function initials(name: string): string {
  return name
    .trim()
    .split(/\s+/)
    .map((word) => (/^\d+$/.test(word) ? word : word[0].toUpperCase()))
    .join("")
    .slice(0, 4);
}

/**
 * One patched fixture. Expanded: inline-renamable name, delete, the color
 * wheel, and every channel. Collapsed: just the live essentials — the name's
 * initials, the color wheel, and the Brightness fader (when the fixture has
 * one); everything editorial waits behind expand.
 */
export default function FixtureCard({
  fixture,
  buffer,
  vertical,
  collapsed,
}: {
  fixture: Fixture;
  buffer: number[] | null;
  vertical: boolean;
  collapsed: boolean;
}) {
  const { id, name, address, channels } = fixture;
  const hasColor = COLOR_ROLES.every((role) =>
    channels.some((c) => c.role === role)
  );
  const dimmerIndex = channels.findIndex((c) => c.role === "Brightness");

  // Collapse state persists (device-local, in setups.json); the view owns the
  // source of truth and this just requests the flip.
  const setCollapsed = (next: boolean) =>
    setFixtureCollapsed(id, next).catch((e) => toast.error(String(e)));
  // The collapse axis follows the layout: the vertical console shrinks a card
  // sideways, the horizontal list shrinks it downward — the icons say which.
  const CollapseIcon = vertical ? ChevronsRightLeft : ChevronsDownUp;
  const ExpandIcon = vertical ? ChevronsLeftRight : ChevronsUpDown;

  const refresh = useLuxRefresh();

  const removeFixture = () =>
    createTauRPCProxy()
      .cmd.remove_fixture(id)
      .then(refresh)
      .catch((e) => toast.error(String(e)));

  // Inline-editable name; persists on blur/Enter, reverts on Escape.
  const [draft, setDraft] = useState(name);
  useEffect(() => setDraft(name), [name]);

  const rename = (next: string) => {
    const trimmed = next.trim();
    if (!trimmed || trimmed === name) {
      setDraft(name);
      return;
    }
    createTauRPCProxy()
      .cmd.update_fixture(id, trimmed, address, channels)
      .then(refresh)
      .catch((e) => toast.error(String(e)));
  };

  // Inline-editable start channel, same interaction as the name. The channel
  // count comes from the channel defs, so the range moves as one block; the
  // backend validates bounds and overlaps and its message surfaces as a toast.
  const [addressDraft, setAddressDraft] = useState(String(address));
  useEffect(() => setAddressDraft(String(address)), [address]);

  const readdress = (next: string) => {
    const parsed = Number.parseInt(next.trim(), 10);
    if (!Number.isFinite(parsed) || parsed < 1 || parsed === address) {
      setAddressDraft(String(address));
      return;
    }
    createTauRPCProxy()
      .cmd.update_fixture(id, name, parsed, channels)
      .then(refresh)
      .catch((e) => {
        setAddressDraft(String(address));
        toast.error(String(e));
      });
  };

  // The span's end previews the draft while it's a plausible start.
  const parsedDraft = Number.parseInt(addressDraft, 10);
  const previewStart =
    Number.isFinite(parsedDraft) && parsedDraft >= 1 ? parsedDraft : address;
  const previewEnd = previewStart + channels.length - 1;

  if (collapsed) {
    const expandButton = (
      <button
        type="button"
        onClick={() => setCollapsed(false)}
        aria-expanded={false}
        aria-label={`Expand ${name}`}
        title={name}
        className="flex items-center gap-1 font-semibold transition-colors hover:text-muted-foreground"
      >
        {initials(name)}
        <ExpandIcon className="size-3.5 text-muted-foreground/60" />
      </button>
    );
    const dimmer = dimmerIndex >= 0 && (
      <FixtureChannel
        address={address + dimmerIndex}
        role={channels[dimmerIndex].role}
        label={channels[dimmerIndex].label}
        value={buffer?.[address + dimmerIndex - 1] ?? 0}
        vertical={vertical}
        hideLabel
      />
    );

    if (vertical) {
      return (
        <section className="w-fit shrink-0 rounded-xl border bg-card p-3">
          <div className="flex flex-col items-center gap-2">
            {expandButton}
            {/* pl-2 cancels the trigger's -ml-2 so the swatch centers. */}
            {hasColor && (
              <div className="pl-2">
                <FixtureColor fixture={fixture} buffer={buffer} label="" />
              </div>
            )}
            {dimmer}
          </div>
        </section>
      );
    }
    return (
      <section className="rounded-xl border bg-card px-5 py-3">
        <div className="flex items-center gap-3">
          {expandButton}
          {hasColor && (
            <FixtureColor fixture={fixture} buffer={buffer} label="" />
          )}
          <div className="min-w-0 flex-1">{dimmer}</div>
        </div>
      </section>
    );
  }

  return (
    // Vertical mode: the card hugs its fader strips instead of stretching to
    // the container, so cards pack side by side in the scrolling bank.
    <section
      className={cn(
        "rounded-xl border bg-card p-5",
        vertical && "w-fit shrink-0"
      )}
    >
      <header className="mb-3 flex items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <input
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={() => rename(draft)}
            onKeyDown={(e) => {
              if (e.key === "Enter") e.currentTarget.blur();
              if (e.key === "Escape") {
                setDraft(name);
                e.currentTarget.blur();
              }
            }}
            aria-label="Fixture name"
            className="-ml-1 w-full truncate rounded-sm border border-transparent bg-transparent px-1 font-semibold outline-none transition-colors hover:border-border focus:border-border"
          />
          <p className="flex items-center px-1 text-xs tabular-nums text-muted-foreground">
            {channels.length === 1 ? "Channel" : "Channels"}
            <input
              value={addressDraft}
              onChange={(e) => setAddressDraft(e.target.value)}
              onBlur={() => readdress(addressDraft)}
              onKeyDown={(e) => {
                if (e.key === "Enter") e.currentTarget.blur();
                if (e.key === "Escape") {
                  setAddressDraft(String(address));
                  e.currentTarget.blur();
                }
              }}
              inputMode="numeric"
              aria-label="Fixture start channel"
              className="ml-1 w-9 rounded-sm border border-transparent bg-transparent px-1 text-xs tabular-nums outline-none transition-colors hover:border-border focus:border-border"
            />
            {channels.length > 1 && <span>-{previewEnd}</span>}
          </p>
        </div>
        <Button
          variant="ghost"
          size="icon"
          className="size-8 shrink-0 text-muted-foreground/60 hover:text-foreground"
          aria-label={`Collapse ${name}`}
          aria-expanded
          onClick={() => setCollapsed(true)}
        >
          <CollapseIcon className="size-4" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          className="size-8 shrink-0 text-muted-foreground/60 hover:text-foreground"
          aria-label={`Remove ${name}`}
          onClick={() => removeFixture()}
        >
          <Trash2 className="size-4" />
        </Button>
      </header>

      {hasColor && (
        <div className="mb-1">
          <FixtureColor fixture={fixture} buffer={buffer} />
        </div>
      )}

      <div className={vertical ? "flex gap-1" : "flex flex-col gap-0.5"}>
        {channels.map((channel, i) => (
          <FixtureChannel
            key={`${id}-${i}`}
            address={address + i}
            role={channel.role}
            label={channel.label}
            value={buffer?.[address + i - 1] ?? 0}
            vertical={vertical}
          />
        ))}
      </div>
    </section>
  );
}
