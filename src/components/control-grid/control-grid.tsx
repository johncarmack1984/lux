"use client";

import { Channel } from "@/components/control-grid/channel";
import {
  Table,
  TableBody,
  TableHead,
  TableHeader,
} from "@/components/ui/table";
import { useCallback, useEffect } from "react";

import useBuffer from "@/hooks/useBuffer";
import useChannelData from "@/hooks/useChannelData";
import GridCaption from "./grid-caption";
import { invoke } from "@tauri-apps/api/core";
import { cva } from "class-variance-authority";

const controlGridVariants = cva("", {
  variants: {
    orientation: {
      vertical: "flex-col",
      horizontal: "flex-row",
    },
  },
});

export default function ControlGrid() {
  const luxChannels = useChannelData();
  const buffer = useBuffer();

  useEffect(() => {
    invoke("sync_state");
  }, []);

  const GridBody = useCallback(
    ({ buffer }: { buffer: number[] | null }) => (
      <TableBody>
        {luxChannels?.map((channel) => {
          return (
            <Channel
              key={channel.channel_number}
              value={buffer?.[channel.channel_number - 1] ?? 0}
              {...channel}
            />
          );
        })}
      </TableBody>
    ),
    [luxChannels]
  );

  return (
    <Table className="caption-top">
      <GridCaption />
      <GridBody buffer={buffer} />
    </Table>
  );
}
