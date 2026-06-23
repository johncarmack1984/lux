"use client";

import { Popover, PopoverContent } from "@/components/ui/popover";

import useBuffer from "@/hooks/useBuffer";
import { useEffect, useState } from "react";
import { RgbaColorPicker, type RgbaColor } from "react-colorful";
import { cn } from "@/lib/utils";
import { bufferToRgba, defaultBuffer, rgbaToBuffer } from "./rgb-utils";
import RgbaInput from "./rgba-input";
import ColorTrigger from "./color-trigger";
import { createTauRPCProxy } from "@/bindings";
import useThrottle from "@/hooks/useThrottle";

const ColorPicker = ({ className }: { className?: string }) => {
  const buffer = useBuffer();

  const [color, setColor] = useState<RgbaColor>(
    bufferToRgba(buffer ?? defaultBuffer)
  );

  useEffect(() => {
    if (!buffer) return;
    setColor(bufferToRgba(buffer));
  }, [buffer]);

  // The color wheel fires continuously; keep the swatch immediate but throttle
  // the buffer write to the hardware/IPC path.
  const sendBuffer = useThrottle((next: RgbaColor) => {
    createTauRPCProxy().cmd.set_buffer(rgbaToBuffer(next));
  }, 40);
  const selectColor = (newColor: RgbaColor) => {
    setColor(newColor);
    sendBuffer(newColor);
  };

  return (
    <Popover>
      <ColorTrigger className={cn(className)} color={color} />
      <PopoverContent className="">
        <RgbaColorPicker
          className="mx-auto"
          color={color}
          onChange={selectColor}
        />
        <RgbaInput color={color} onChange={selectColor} />
      </PopoverContent>
    </Popover>
  );
};

export default ColorPicker;
