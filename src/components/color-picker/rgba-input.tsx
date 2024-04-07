"use client";

import { type ChangeEvent } from "react";
import { type RgbaColor } from "react-colorful";
import { Input, inputVariants } from "../ui/input";
import { cn } from "@/lib/utils";
import { Label } from "../ui/label";

const RgbaInput = ({
  color,
  onChange,
}: {
  color: RgbaColor;
  onChange: (newColor: RgbaColor) => void;
}) => {
  const inputColor = (e: ChangeEvent<HTMLInputElement>) => {
    const { name, value } = e.target;
    onChange({ ...color, [name]: +value });
  };

  return (
    <div className="flex justify-between">
      {["r", "g", "b", "a"].map((key) => {
        const isAlpha = key === "a";
        const raw = color[key as keyof RgbaColor];
        const value = isAlpha ? raw.toFixed(2) : raw;
        return (
          <div key={key} className="text-center">
            <Label className="mr-2 uppercase">{key}</Label>
            <Input
              name={key}
              type="number"
              min={0}
              max={isAlpha ? 1 : 255}
              maxLength={isAlpha ? 4 : 3}
              pattern={isAlpha ? "[0-1]{1}.[0-9]{2}" : "\\d{3}"}
              step={isAlpha ? 0.01 : 1}
              value={value}
              onChange={inputColor}
              className={cn(inputVariants({ variant: "color" }), isAlpha && "")}
            />
          </div>
        );
      })}
    </div>
  );
};

export default RgbaInput;
