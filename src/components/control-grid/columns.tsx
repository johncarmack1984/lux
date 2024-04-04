"use client";

import type { ChannelProps, LightColorVariants, LuxLabelColor } from "@/global";
import {
  createColumnHelper,
  type ColumnDef,
  type CellContext,
} from "@tanstack/react-table";
import { ActionsMenu } from "./action-menu";
import { cn, lightColorVariants } from "@/lib/utils";
import { Input } from "@/components/ui/input";
import { Button } from "../ui/button";
import { use, useEffect, useState } from "react";
import { Slider } from "@/components/ui/slider";
import { TableCell } from "../ui/table";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { setChannelMetadata, setChannelValue } from "@/app/actions";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";

const columnHelper = createColumnHelper<ChannelProps>();

const ChannelLabel = ({ row }: CellContext<ChannelProps, unknown>) => {
  const { label, id } = row.original;
  const key = `channel-label-${id}`;
  return (
    <TableCell id={key} key={key}>
      <Input
        name="label"
        className="bg-transparent border-transparent max-w-24 text-right"
        defaultValue={label}
      />
    </TableCell>
  );
};

const labelColorOptions: LuxLabelColor[] = [
  "Red",
  "Green",
  "Blue",
  "Amber",
  "White",
  "Brightness",
];

const ColorOption = (label_color: LuxLabelColor) => {
  const firstLetter = label_color[0].toUpperCase();
  return (
    <DropdownMenuItem
      className="flex justify-end gap-4 w-full text-right"
      key={`${label_color}-dropdown-item`}
    >
      {label_color}
      <Button className={cn(lightColorVariants({ label_color }))} size="icon">
        {firstLetter}
      </Button>
    </DropdownMenuItem>
  );
};

const ChannelNumber = ({ row }: CellContext<ChannelProps, unknown>) => {
  const { channel_number, label_color } = row.original;
  const key = `channel-number-${row.original.id}`;
  return (
    <TableCell className="w-5" id={key} key={key}>
      <DropdownMenu>
        <DropdownMenuTrigger>
          <div className={cn(lightColorVariants({ label_color }))}>
            {channel_number}
          </div>
        </DropdownMenuTrigger>
        <DropdownMenuContent className=" w-40" align="end">
          {labelColorOptions.map(ColorOption)}
        </DropdownMenuContent>
      </DropdownMenu>
    </TableCell>
  );
};

const ChannelValue = ({ row }: CellContext<ChannelProps, unknown>) => {
  const { id, value } = row.original;
  const key = `channel-value-${id}`;
  const toggle = async () => {
    const newValue = value === 0 ? 255 : 0;
    await setChannelValue({
      channelNumber: row.original.channel_number,
      value: newValue,
    }).catch(toast.error);
  };
  return (
    <TableCell className="w-14" key={key} id={key}>
      <Button onClick={toggle} variant="outline" size="sm">
        {value.toString().padStart(3, "0")}
      </Button>
    </TableCell>
  );
};

const ChannelSlider = ({ row }: CellContext<ChannelProps, unknown>) => {
  const { id, channel_number: channelNumber, value } = row.original;
  const [values, setValues] = useState([value]);
  const dragSlider = async (newValues: number[]) => {
    setValues(newValues);
    await setChannelValue({ channelNumber, value: newValues[0] });
  };
  useEffect(() => {
    setValues([value]);
  }, [value]);

  const key = `value-slider-${id}`;
  return (
    <TableCell className="w-full" id={key} key={key}>
      <Slider
        id={`${key}-slider`}
        value={values}
        onValueChange={dragSlider}
        max={255}
        step={1}
      />
    </TableCell>
  );
};

const columns: ColumnDef<ChannelProps>[] = [
  {
    accessorKey: "label",
    cell: ChannelLabel,
  },
  {
    accessorKey: "channel_number",
    cell: ChannelNumber,
  },
  {
    id: "value_button",
    accessorKey: "value",
    cell: ChannelValue,
  },
  {
    id: "value_slider",
    accessorKey: "value",
    cell: ChannelSlider,
  },
  columnHelper.display({
    id: "actions",
    cell: ActionsMenu,
  }),
];

export default columns;
