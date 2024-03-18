"use client";
import Channel from "@/components/channel";
import Greeting from "@/components/greeting";
import ButtonRow from "@/components/button-row";
import { type ChannelType, channels } from "@/lib/utils";
import { Table, TableBody } from "@/components/ui/table";
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/tauri";
import { listen } from "@tauri-apps/api/event";
import { error, attachConsole } from "tauri-plugin-log-api";

type LuxSystemStateEvent = {
  event: string;
  windowLabel: string;
  payload: {
    buffer: number[];
  };
  id: number;
};

const detach = async () => await attachConsole();

export default function Home() {
  const [buffer, setBuffer] = useState<
    LuxSystemStateEvent["payload"]["buffer"]
  >(Array(channels.length).fill(0));

  const setupListeners = useCallback(async () => {
    await listen("system_state_update", ({ payload }: LuxSystemStateEvent) => {
      setBuffer(payload.buffer);
    });
  }, []);

  setupListeners();

  const bufferToSliders = (c: ChannelType, i: number) => ({
    ...c,
    value: buffer[i],
  });

  return (
    <main className="flex min-h-screen flex-col items-center justify-between py-8 px-12">
      <Greeting />
      <ButtonRow />
      <Table>
        <TableBody>{channels.map(bufferToSliders).map(Channel)}</TableBody>
      </Table>
    </main>
  );
}
