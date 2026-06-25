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
 * no boxed value button. The value is a quiet 0/full toggle.
 */
export default function FixtureChannel({
  address,
  role,
  label,
  value,
}: {
  address: number;
  role: LuxLabelColor;
  label: string;
  value: number;
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

  return (
    <div className="flex items-center gap-3 py-1.5">
      <span className="w-4 text-right text-xs tabular-nums text-muted-foreground/60">
        {address}
      </span>
      <span className={cn("size-2.5 shrink-0 rounded-full", ROLE_DOT[role])} />
      <span className="w-16 shrink-0 truncate text-sm">{label}</span>
      <Slider
        aria-label={`${label} (channel ${address})`}
        value={values}
        onValueChange={drag}
        max={255}
        step={1}
        className="flex-1"
      />
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
