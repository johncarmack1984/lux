import { useEffect, useState } from "react";
import { toast } from "sonner";
import type { LuxLabelColor } from "@/bindings";
import { Slider } from "@/components/ui/slider";
import { cn } from "@/lib/utils";
import { setChannelValue } from "@/lib/actions";
import useThrottle from "@/hooks/useThrottle";

// Just the fill — a small role dot, not the heavy bordered badge the desk uses.
const ROLE_DOT: Record<LuxLabelColor, string> = {
  Red: "bg-red-500",
  Green: "bg-green-500",
  Blue: "bg-blue-500",
  Amber: "bg-amber-300",
  White: "bg-zinc-100",
  Brightness: "bg-zinc-400",
  Generic: "bg-zinc-600",
};

/**
 * One channel inside a fixture card: address, role dot, label, slider, value.
 * Deliberately lighter than the universe desk's row — no bordered number badge,
 * no boxed value button. The value is a quiet 0/full toggle. Follows the
 * "slider orientation" setting: a horizontal row, or a compact vertical fader
 * strip (same element order as the desk's column: meta above, fader below).
 */
export default function FixtureChannel({
  address,
  role,
  label,
  value,
  vertical = false,
}: {
  address: number;
  role: LuxLabelColor;
  label: string;
  value: number;
  vertical?: boolean;
}) {
  const [values, setValues] = useState([value]);
  useEffect(() => setValues([value]), [value]);

  const send = useThrottle((next: number) => {
    setChannelValue({ channelNumber: address, value: next }).catch((e) =>
      toast.error(String(e))
    );
  }, 40);

  const drag = (next: number[]) => {
    setValues(next);
    send(next[0]);
  };

  const toggle = () => {
    const next = values[0] === 0 ? 255 : 0;
    setValues([next]);
    send(next);
  };

  const slider = (
    <Slider
      orientation={vertical ? "vertical" : "horizontal"}
      aria-label={`${label} (channel ${address})`}
      value={values}
      onValueChange={drag}
      max={255}
      step={1}
      className={vertical ? undefined : "flex-1"}
    />
  );

  if (vertical) {
    return (
      <div className="flex h-full w-14 shrink-0 flex-col items-center gap-1.5 py-1">
        <span className="text-xs tabular-nums text-muted-foreground/60">
          {address}
        </span>
        <span className={cn("size-2.5 shrink-0 rounded-full", ROLE_DOT[role])} />
        <span className="w-full truncate text-center text-xs">{label}</span>
        <button
          type="button"
          onClick={toggle}
          title="Toggle 0 / full"
          className="text-xs tabular-nums text-muted-foreground transition-colors hover:text-foreground"
        >
          {values[0].toString().padStart(3, "0")}
        </button>
        {/* h-36 is the fader's base height AND the definite box the slider's
            internal h-full resolves against (a min-height-derived height is
            not "definite" to WebKit, which zeroes the track). `grow` — not
            flex-1, whose basis:0 would override the height — lets the fader
            fill taller cards (a collapsed card stretched level with its
            expanded neighbors). */}
        <div className="h-36 grow pt-1">{slider}</div>
      </div>
    );
  }

  return (
    <div className="flex items-center gap-3 py-1.5">
      <span className="w-4 text-right text-xs tabular-nums text-muted-foreground/60">
        {address}
      </span>
      <span className={cn("size-2.5 shrink-0 rounded-full", ROLE_DOT[role])} />
      <span className="w-16 shrink-0 truncate text-sm">{label}</span>
      {slider}
      <button
        type="button"
        onClick={toggle}
        title="Toggle 0 / full"
        className="w-9 text-right text-xs tabular-nums text-muted-foreground transition-colors hover:text-foreground"
      >
        {values[0].toString().padStart(3, "0")}
      </button>
    </div>
  );
}
