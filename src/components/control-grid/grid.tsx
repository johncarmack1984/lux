"use client";

import { type ChannelProps } from "@/global";
import { Table, TableBody, TableCell, TableRow } from "@/components/ui/table";

import GridFooter from "./footer";
import columns from "./columns";
import {
  getCoreRowModel,
  useReactTable,
  type Table as TableType,
} from "@tanstack/react-table";
import { deleteChannel, editChannel } from "@/app/actions";
import GridRow from "./row";
import useLuxState from "@/hooks/useLuxState";

function EmptyGrid() {
  const key = "no-rows-present-row";
  return (
    <TableRow key={key} id={key}>
      <TableCell
        key={`${key}-cell`}
        colSpan={columns.length}
        className="text-center"
      >
        No channels
      </TableCell>
    </TableRow>
  );
}

function GridBody({ table }: { table: TableType<ChannelProps> }) {
  const areRowsPresent = table.getRowModel().rows.length;
  return (
    <TableBody>
      {areRowsPresent ? table.getRowModel().rows.map(GridRow) : <EmptyGrid />}
    </TableBody>
  );
}

export default function ControlGrid() {
  const data = useLuxState();

  const table = useReactTable({
    data,
    columns,
    getCoreRowModel: getCoreRowModel(),
    meta: {
      editChannel,
      deleteChannel,
    },
  });

  return (
    <Table>
      <GridBody table={table} />
      <GridFooter />
    </Table>
  );
}
