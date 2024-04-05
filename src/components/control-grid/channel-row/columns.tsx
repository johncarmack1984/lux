"use client";

import type { ChannelProps } from "@/global";
import { createColumnHelper, type ColumnDef } from "@tanstack/react-table";
import { ActionsMenu } from "../action-menu";
import ChannelLabel from "./channel-label";
import ChannelNumber from "./channel-number";
import ChannelValue from "./channel-value";
import ChannelSlider from "./channel-slider";

const columnHelper = createColumnHelper<ChannelProps>();

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
