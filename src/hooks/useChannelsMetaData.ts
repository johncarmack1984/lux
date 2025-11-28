import type { LuxChannel } from "@/global";
import { trace } from "@tauri-apps/plugin-log";
import { useState, useEffect } from "react";
import { createTauRPCProxy } from "../../bindings";

function useChannelData() {
  const [channelData, setChannelData] = useState<LuxChannel[] | null>(null);
  useEffect(() => {
    trace(`useChannel useEffect`);
    const taurpc = createTauRPCProxy();
    const unlisten = taurpc.cmd.channel_data_set.on((channels) => {
      trace(`useChannel listen ${channels.map((c) => c.label).join(", ")}`);
      setChannelData(channels);
    });

    return () => {
      trace(`useChannel return`);
      unlisten.then((f) => f());
    };
  }, []);
  return channelData;
}

export default useChannelData;
