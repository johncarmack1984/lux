"use client";

import { Slider } from "../ui/slider";
import { Button } from "../ui/button";
import { TableCell, TableRow } from "../ui/table";
import { debug } from "@tauri-apps/plugin-log";
import { invoke } from "@tauri-apps/api/core";
import type { ChannelProps } from "@/global";
import { cn, lightColorVariants } from "@/lib/utils";

async function setChannel({
  channelNumber,
  value,
}: {
  channelNumber: number;
  value: number;
}) {
  return await invoke("update_channel_value", {
    channelNumber,
    value,
  });
}

function Channel({ label, channel_number, label_color, value }: ChannelProps) {
  const channelNumber = channel_number;
  const id = `channel-${channelNumber}-${label}-slider`;

  if (typeof value !== "number") return null;

  const handleValueChange = async (newValue: number[]) => {
    setChannel({ channelNumber, value: newValue[0] });
  };

  const toggle = () => {
    debug(`togggle ${channelNumber}`);
    setChannel({ channelNumber, value: value > 0 ? 0 : 255 });
  };

  return (
    <TableRow key={`${label}-${label_color}-${channelNumber}`}>
      <TableCell className="w-5">
        <div className={cn(lightColorVariants({ label_color }))}>
          {channelNumber}
        </div>
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
          onValueChange={handleValueChange}
          max={255}
          step={1}
        />
      </TableCell>
    </TableRow>
  );
}

export { Channel, type ChannelProps };
