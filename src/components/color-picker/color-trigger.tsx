"use client";

import { type RgbaColor } from "react-colorful";
import { Button } from "../ui/button";
import { cn } from "@/lib/utils";
import { PopoverTrigger } from "../ui/popover";

function ColorTrigger({
  className,
  color,
}: {
  className?: string;
  color: RgbaColor;
}) {
  const backgroundColor = `rgba(${color.r}, ${color.g}, ${color.b}, ${color.a})`;
  return (
    <PopoverTrigger className={cn(className)} asChild>
      <Button variant="default" className={cn("gap-3", className)}>
        Fixture Color
        <div
          className="rounded-full size-7 border-[1px] border-border/50 "
          style={{ backgroundColor }}
        />
      </Button>
    </PopoverTrigger>
  );
}

export default ColorTrigger;
