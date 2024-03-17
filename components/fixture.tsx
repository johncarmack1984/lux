"use client";

import { invoke } from "@tauri-apps/api/tauri";
import { Slider } from "./ui/slider";
import { useEffect, useState } from "react";
import { Label } from "./ui/label";
import { Button } from "./ui/button";

function Fixture({ label, channel }: { label: string; channel: number }) {
  const id = `channel-${channel}-${label}-slider`;
  const [values, setValues] = useState<number[]>([0]);

  useEffect(() => {
    const value = values[0];
    invoke("slider", { channel, value });
  }, [channel, values]);

  const toggle = () => {
    const value = values[0] === 0 ? 255 : 0;
    setValues([value]);
  };

  return (
    <div className="flex items-center w-full space-x-2">
      <Label className="flex gap-2 items-center" htmlFor={id}>
        <span>{label}</span>
        <Button onClick={toggle} className="" variant="outline" size="sm">
          {values[0]}
        </Button>
      </Label>
      <Slider
        id={id}
        value={values}
        onValueChange={setValues}
        max={255}
        step={1}
      />
    </div>
  );
}

export default Fixture;
