"use client";

import { Popover, PopoverContent } from "@/components/ui/popover";

import useBuffer from "@/hooks/useBuffer";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState, type ChangeEvent } from "react";
import { RgbaColorPicker, HexColorInput, type RgbaColor } from "react-colorful";
import { cn } from "@/lib/utils";
import { bufferToRgba, defaultBuffer, rgbaToBuffer } from "./rgb-utils";
import RgbaInput from "./rgba-input";
import ColorTrigger from "./color-trigger";

const ColorPicker = ({ className }: { className?: string }) => {
  const buffer = useBuffer();

  const [color, setColor] = useState<RgbaColor>(
    bufferToRgba(buffer ?? defaultBuffer)
  );

  useEffect(() => {
    if (!buffer) return;
    setColor(bufferToRgba(buffer));
  }, [buffer]);

  const selectColor = (newColor: RgbaColor) => {
    setColor(newColor);
    invoke("set_buffer", { buffer: rgbaToBuffer(newColor) });
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
