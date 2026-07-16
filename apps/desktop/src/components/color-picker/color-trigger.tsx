import { colord } from "colord";
import { type RgbaColor } from "react-colorful";
import { Button } from "../ui/button";
import { cn } from "@/lib/utils";
import { PopoverTrigger } from "../ui/popover";

/**
 * The color swatch that opens the picker. The swatch glows: a radial-gradient
 * core plus a luminance-scaled box-shadow, so a brighter fixture literally casts
 * more light. `luminance` (0..1) is supplied by the caller — the fixture's dimmer
 * channel, or its brightest color channel when there is no dimmer.
 */
export default function ColorTrigger({
  className,
  color,
  luminance,
  label = "Color",
}: {
  className?: string;
  color: RgbaColor;
  luminance: number;
  label?: string;
}) {
  const spread = Math.round(luminance * 100);
  const fill = `rgba(${color.r},${color.g},${color.b},${color.a})`;
  const light = colord(fill).lighten(luminance).toRgbString();
  const background = `radial-gradient(${light} 0, ${fill} ${spread}%)`;
  const boxShadow = `0 0 ${spread}px ${light}`;
  // label="" renders a bare swatch (compact contexts): sized to the minimum
  // touch target (44px) with zero padding, so the hit area is exactly the
  // visible circle — no invisible margin to open the picker by accident.
  const bare = !label;
  return (
    <PopoverTrigger asChild>
      <Button
        variant="ghost"
        aria-label={label || "Color"}
        className={cn(bare ? "size-11 rounded-full p-0" : "gap-3", className)}
      >
        {label || null}
        {/* The faint ring keeps the swatch findable at blackout (a black
            glow-less circle would vanish into the card). */}
        <div
          className={cn(
            "rounded-full border border-border/60",
            bare ? "size-11" : "size-7"
          )}
          style={{ background, boxShadow }}
        />
      </Button>
    </PopoverTrigger>
  );
}
