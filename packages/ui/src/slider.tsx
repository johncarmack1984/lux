import * as React from "react";
import { Slider as BaseSlider } from "@base-ui/react/slider";
import { cn } from "./cn";

/* The app's channel slider: thin track, white fill, white round thumb —
   exactly the control lux renders for every channel of the universe. */

export interface FaderProps {
  value: number;
  onValueChange: (value: number) => void;
  min?: number;
  max?: number;
  step?: number;
  "aria-label": string;
  className?: string;
}

export function Fader({
  value,
  onValueChange,
  min = 0,
  max = 255,
  step = 1,
  className,
  ...aria
}: FaderProps) {
  return (
    <BaseSlider.Root
      value={value}
      onValueChange={(next) =>
        onValueChange(Array.isArray(next) ? next[0] : next)
      }
      min={min}
      max={max}
      step={step}
      className={cn("relative flex w-full touch-none select-none", className)}
    >
      <BaseSlider.Control className="flex h-6 w-full items-center">
        <BaseSlider.Track className="relative h-[3px] w-full grow rounded-full bg-[#2a2f37]">
          <BaseSlider.Indicator className="rounded-full bg-[#d7dbe0]" />
          <BaseSlider.Thumb
            aria-label={aria["aria-label"]}
            className="block size-[18px] rounded-full border border-black/50 bg-white shadow-[0_1px_2px_rgb(0_0_0/0.5)]"
          />
        </BaseSlider.Track>
      </BaseSlider.Control>
    </BaseSlider.Root>
  );
}
