import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Trash2 } from "lucide-react";
import { createTauRPCProxy, type Fixture } from "@/bindings";
import useLuxRefresh from "@/hooks/useLuxRefresh";
import { Button } from "@/components/ui/button";
import FixtureChannel from "./fixture-channel";
import FixtureColor from "./fixture-color";

const COLOR_ROLES = ["Red", "Green", "Blue"] as const;

export default function FixtureCard({
  fixture,
  buffer,
}: {
  fixture: Fixture;
  buffer: number[] | null;
}) {
  const { id, name, address, channels } = fixture;
  const end = address + channels.length - 1;
  const hasColor = COLOR_ROLES.every((role) =>
    channels.some((c) => c.role === role)
  );

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

  const span =
    channels.length === 1 ? `Channel ${address}` : `Channels ${address}-${end}`;

  return (
    <section className="rounded-xl border bg-card p-5">
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
          <p className="px-1 text-xs tabular-nums text-muted-foreground">
            {span}
          </p>
        </div>
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

      <div className="flex flex-col gap-0.5">
        {channels.map((channel, i) => (
          <FixtureChannel
            key={`${id}-${i}`}
            address={address + i}
            role={channel.role}
            label={channel.label}
            value={buffer?.[address + i - 1] ?? 0}
          />
        ))}
      </div>
    </section>
  );
}
