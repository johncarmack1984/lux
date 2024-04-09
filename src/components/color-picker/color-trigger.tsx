"use client";

import { colord } from "colord";
import { type RgbaColor } from "react-colorful";
import { Button } from "../ui/button";
import { cn } from "@/lib/utils";
import { PopoverTrigger } from "../ui/popover";
import useBuffer from "@/hooks/useBuffer";
import { bufferToLuminance } from "./rgb-utils";

function ColorTrigger({
  className,
  color,
}: {
  className?: string;
  color: RgbaColor;
}) {
  const buffer = useBuffer();
  const luminance = bufferToLuminance(buffer);
  const origin = 0;
  const destination = Math.round(luminance * 100);
  const backgroundColor = `rgba(${color.r},${color.g},${color.b},${color.a})`;
  const lightColor = colord(backgroundColor).lighten(luminance).toRgbString();
  const boxShadow = `0 0 ${destination}px ${lightColor}`;
  return (
    <PopoverTrigger className={cn(className)} asChild>
      <Button variant="ghost" className={cn("gap-3", className)}>
        Fixture Color
        <div
          className="rounded-full size-7"
          style={{
            background: `radial-gradient(${lightColor} ${origin}, ${backgroundColor} ${destination}%)`,
            backgroundColor,
            boxShadow,
          }}
        />
      </Button>
    </PopoverTrigger>
  );
}

export default ColorTrigger;
