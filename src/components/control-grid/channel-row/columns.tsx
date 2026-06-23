"use client";

import type { ChannelProps } from "@/global";
import { type ColumnDef } from "@tanstack/react-table";
import ChannelLabel from "./channel-label";
import ChannelNumber from "./channel-number";
import ChannelValue from "./channel-value";
import ChannelSlider from "./channel-slider";

const columns: ColumnDef<ChannelProps>[] = [
  {
    accessorKey: "label",
    cell: ChannelLabel,
  },
  {
    accessorKey: "channelNumber",
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
];

export default columns;
