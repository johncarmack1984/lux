"use client";

import type { ChannelProps, LuxLabelColor } from "@/global";
import { type CellContext } from "@tanstack/react-table";
import { cn, lightColorVariants } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { TableCell } from "@/components/ui/table";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

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

export default ChannelNumber;
