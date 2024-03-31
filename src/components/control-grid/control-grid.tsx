"use client";

import Database from "@tauri-apps/plugin-sql";

import { Channel } from "@/components/control-grid/channel";
import { Table, TableBody } from "@/components/ui/table";
import { useCallback, useEffect } from "react";

import useBuffer from "@/hooks/useBuffer";
import useChannelData from "@/hooks/useChannelData";
import GridCaption from "./grid-caption";
import { invoke } from "@tauri-apps/api/core";

export default function ControlGrid() {
  const luxChannels = useChannelData();
  const buffer = useBuffer();

  // sqlite. The path is relative to `tauri::api::path::BaseDirectory::App`.
  let db = useCallback(async () => await Database.load("sqlite:test.db"), []);
  useEffect(() => {
    db();
  }, [db]);

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
