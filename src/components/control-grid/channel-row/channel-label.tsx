"use client";

import type { ChannelProps } from "@/global";
import { type CellContext } from "@tanstack/react-table";
import { Input } from "@/components/ui/input";
import { TableCell } from "../../ui/table";

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

export default ChannelLabel;
