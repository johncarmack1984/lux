"use client";

import Channel from "@/components/channel";
import { type LuxChannel, channels } from "@/lib/utils";
import { Table, TableBody, TableCaption } from "@/components/ui/table";
import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { trace, attachConsole, info, debug } from "tauri-plugin-log-api";

type LuxSystemStateEvent = {
  event: string;
  windowLabel: string;
  payload: {
    buffer: number[];
    channels: LuxChannel[];
  };
  id: number;
};

const detach = async () => await attachConsole();

export default function ControlGrid() {
  const [luxChannels, setChannels] = useState<LuxChannel[]>(channels);

  const setupListeners = useCallback(async () => {
    await listen("system_state_update", ({ payload }: LuxSystemStateEvent) => {
      debug(`system_state_update payload { ${JSON.stringify(payload)} }`);
      setChannels(payload.channels);
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
      <TableBody>{luxChannels.map(Channel)}</TableBody>
    </Table>
  );
}
