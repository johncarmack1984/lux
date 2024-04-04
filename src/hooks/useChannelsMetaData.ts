import type { LuxChannel } from "@/global";
import { listen } from "@tauri-apps/api/event";
import { attachConsole, trace } from "@tauri-apps/plugin-log";
import { useState, useEffect, useCallback } from "react";
import { toast } from "sonner";
import { set } from "zod";

const detach = async () => await attachConsole();

function useChannelData() {
  const [channelData, setChannelData] = useState<LuxChannel[] | null>(null);

  const setupListeners = useCallback(async () => {
    await listen<LuxChannel[]>("channel_data_set", ({ payload }) => {
      trace(`useChannel listen ${payload.map((c) => c.label).join(", ")}`);
      setChannelData(payload);
    }).catch(toast.error);
  }, []);

  useEffect(() => {
    setupListeners();
    return () => {
      detach();
    };
  }, [setupListeners]);

  return channelData;
}

export default useChannelData;
