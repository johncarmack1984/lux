"use client";

import { type CellContext } from "@tanstack/react-table";
import { DeleteIcon, MoreHorizontal } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { ChannelProps } from "@/global";
import { TableCell } from "../ui/table";

const ActionsMenu = ({ row, table }: CellContext<ChannelProps, unknown>) => {
  if (!table.options.meta) return null;
  const { deleteChannel, editChannel } = table.options.meta;
  const { id, channel_number } = row.original;
  const key = `actions-menu-${channel_number}`;

  return (
    <TableCell id={key}>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button size="icon" variant="ghost" className="h-8 w-8 p-0">
            <span className="sr-only">Open menu</span>
            <MoreHorizontal className="h-4 w-4" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem
            className="flex justify-between w-full"
            disabled={true}
            aria-disabled={true}
            onClick={async () => await deleteChannel(id)}
            asChild
          >
            <Button variant="destructive" className="">
              <DeleteIcon className="h-4 w-4 mr-2" />
              Disable Channel
            </Button>
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </TableCell>
  );
};

export { ActionsMenu };
