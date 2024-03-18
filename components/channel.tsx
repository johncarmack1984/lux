"use client";

import { Slider } from "./ui/slider";
import { Label } from "./ui/label";
import { Button } from "./ui/button";
import { LuxChannel, LuxLabelColor, cn } from "@/lib/utils";
import { TableCell, TableRow } from "./ui/table";
import { debug, trace } from "tauri-plugin-log-api";
import { invoke } from "@tauri-apps/api/tauri";
import { useEffect, useState } from "react";
import { cva } from "class-variance-authority";

const lightColor = cva(
  "size-8 border-border border-[1px] flex items-center justify-center rounded-full",
  {
    variants: {
      label_color: {
        Red: "bg-red-500",
        Green: "bg-green-500",
        Blue: "bg-blue-500",
        Amber: "bg-amber-200 text-amber-800",
        White: "bg-white text-black",
        Brightness: "bg-black text-white",
      },
    },
  }
);

function Channel({ label, channel_number, label_color, value }: LuxChannel) {
  const id = `channel-${channel_number}-${label}-slider`;

  const [values, setValues] = useState([value]);

  useEffect(() => {
    debug(`channel ${channel_number} useEffect: value to to ${values[0]}`);
    invoke("update_channel_value", {
      channelNumber: channel_number,
      value: values[0],
    });
  }, [values, channel_number]);

  const toggle = () => {
    debug(`togggle ${channel_number}`);
    setValues([value > 0 ? 0 : 255]);
  };

  return (
    <TableRow key={`${label}-${label_color}-${channel_number}`}>
      <TableCell className="w-5">
        <div className={cn(lightColor({ label_color }))}>{channel_number}</div>
      </TableCell>
      <TableCell className=" w-fit">{label}</TableCell>
      <TableCell className="w-14">
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
