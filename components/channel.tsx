"use client";

import { Slider } from "./ui/slider";
import { Label } from "./ui/label";
import { Button } from "./ui/button";
import { lightColors } from "@/lib/utils";
import { TableCell, TableRow } from "./ui/table";
import { info } from "tauri-plugin-log-api";
import { invoke } from "@tauri-apps/api/tauri";
import { useEffect, useState } from "react";

function Channel({
  label = "Channel",
  channel,
  color = "white",
  value,
}: {
  label?: string;
  channel: number;
  color?: keyof typeof lightColors;
  value: number;
}) {
  const id = `channel-${channel}-${label}-slider`;

  const [values, setValues] = useState([value]);

  useEffect(() => {
    info(`channel ${channel} useEffect: value to to ${values[0]}`);
    invoke("update", { channel, value: values[0] });
  }, [values, channel]);

  const toggle = () => {
    info(`togggle ${channel}`);
    setValues([value > 0 ? 0 : 255]);
  };

  return (
    <TableRow key={`${label}-${color}-${channel}`}>
      <TableCell className="w-5">{channel}</TableCell>
      <TableCell className=" w-28">
        <Label className="flex gap-2 items-center" htmlFor={id}>
          {label}
        </Label>
      </TableCell>
      <TableCell className=" w-14">
        <Button onClick={toggle} variant="outline" size="sm">
          {value.toString().padStart(3, "0")}
        </Button>
      </TableCell>
      <TableCell className="w-full">
        <Slider
          id={id}
          value={[value]}
          onValueChange={setValues}
          max={255}
          step={1}
        />
      </TableCell>
    </TableRow>
  );
}

export default Channel;
