"use client";

import { type ChannelProps } from "@/global";
import { TableRow } from "@/components/ui/table";
import { z } from "zod";
import { flexRender, type Row } from "@tanstack/react-table";
import { zodResolver } from "@hookform/resolvers/zod";
import { Fragment } from "react";

function GridRow(row: Row<ChannelProps>) {
  const key = `row-channel-${row.original.channel_number}`;

  return (
    <TableRow id={key} key={key} data-state={row.getIsSelected() && "selected"}>
      {row.getVisibleCells().map((cell) =>
        flexRender(cell.column.columnDef.cell, {
          ...cell.getContext(),
          key: `${key}-cell-${cell.id}`,
        })
      )}
    </TableRow>
  );
}

export default GridRow;
