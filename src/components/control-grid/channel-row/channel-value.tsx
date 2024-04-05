"use client";

import type { ChannelProps } from "@/global";
import { type CellContext } from "@tanstack/react-table";
import { Button } from "../../ui/button";
import { TableCell } from "../../ui/table";
import { setChannelValue } from "@/app/actions";
import { toast } from "sonner";

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

export default ChannelValue;
