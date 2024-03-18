"use client";

import Channel from "@/components/channel";
import { type ChannelType, channels } from "@/lib/utils";
import { Table, TableBody, TableCaption } from "@/components/ui/table";
import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { attachConsole } from "tauri-plugin-log-api";

type LuxSystemStateEvent = {
  event: string;
  windowLabel: string;
  payload: {
    buffer: number[];
  };
  id: number;
};

const detach = async () => await attachConsole();

export default function ControlGrid() {
  const [buffer, setBuffer] = useState<
    LuxSystemStateEvent["payload"]["buffer"]
  >(Array(channels.length).fill(0));

  const bufferToSliders = useCallback(
    (c: ChannelType, i: number) => ({
      ...c,
      value: buffer[i],
    }),
    [buffer]
  );

  const setupListeners = useCallback(async () => {
    await listen("system_state_update", ({ payload }: LuxSystemStateEvent) => {
      setBuffer(payload.buffer);
    });
  }, []);

  useEffect(() => {
    setupListeners();
    return () => {
      detach();
    };
  }, [setupListeners]);

  return (
    <Table className=" caption-top">
      <TableCaption>{JSON.stringify(buffer)}</TableCaption>
      <TableBody>{channels.map(bufferToSliders).map(Channel)}</TableBody>
    </Table>
  );
}
