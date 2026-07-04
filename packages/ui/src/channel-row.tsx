import { Fader } from "./slider";
import { cn } from "./cn";

/* One row of a fixture card, as the app draws it:
   channel number · role dot · label · slider · zero-padded value. */

export interface ChannelRowProps {
  channel: number;
  label: string;
  /** A --lux-ch-* token (or any CSS color) for the role dot. */
  color: string;
  value: number;
  onValueChange: (value: number) => void;
  className?: string;
}

export function ChannelRow({
  channel,
  label,
  color,
  value,
  onValueChange,
  className,
}: ChannelRowProps) {
  return (
    <div className={cn("flex items-center gap-3", className)}>
      <span className="mono w-4 text-right text-sm text-mut">{channel}</span>
      <span
        aria-hidden
        className="size-2.5 shrink-0 rounded-full"
        style={{ background: color }}
      />
      <span className="w-16 shrink-0 text-[15px] font-medium">{label}</span>
      <Fader value={value} onValueChange={onValueChange} aria-label={label} />
      <span className="mono w-[3.5ch] text-right text-sm text-mut">
        {String(value).padStart(3, "0")}
      </span>
    </div>
  );
}
