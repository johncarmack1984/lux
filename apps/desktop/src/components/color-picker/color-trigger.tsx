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
  return (
    <PopoverTrigger asChild>
      <Button variant="ghost" className={cn("gap-3", className)}>
        {label}
        <div className="size-7 rounded-full" style={{ background, boxShadow }} />
      </Button>
    </PopoverTrigger>
  );
}
