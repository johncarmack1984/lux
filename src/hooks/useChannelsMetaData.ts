import type { LuxChannel } from "@/global";
import { listen } from "@tauri-apps/api/event";
import { trace } from "@tauri-apps/plugin-log";
import { useState, useEffect } from "react";

function useChannelData() {
  const [channelData, setChannelData] = useState<LuxChannel[] | null>(null);
  useEffect(() => {
    trace(`useChannel useEffect`);

    const unlisten = listen<LuxChannel[]>("channel_data_set", ({ payload }) => {
      trace(`useChannel listen ${payload.map((c) => c.label).join(", ")}`);
      setChannelData(payload);
    });

    return () => {
      trace(`useChannel return`);
      unlisten.then((f) => f());
    };
  }, []);
  return channelData;
}

export default useChannelData;
