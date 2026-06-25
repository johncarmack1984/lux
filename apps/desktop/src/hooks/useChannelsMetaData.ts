import type { LuxChannel } from "@/global";
import { trace } from "@tauri-apps/plugin-log";
import { useState, useEffect } from "react";
import { createTauRPCProxy } from "@/bindings";

function useChannelData() {
  const [channelData, setChannelData] = useState<LuxChannel[] | null>(null);

  useEffect(() => {
    const taurpc = createTauRPCProxy();
    let active = true;

    // Fetch the current channels directly so the grid populates even if the
    // startup sync event fires before this listener is attached. The listener
    // then keeps it live for later updates.
    taurpc.sync
      .sync_channels()
      .then((c) => {
        if (active) setChannelData(c.channels);
      })
      .catch((e) => trace(`sync_channels failed: ${e}`));

    const unlisten = taurpc.cmd.event.on((event) => {
      if (event.type !== "channelDataSet") return;
      setChannelData(event.channels);
    });

    return () => {
      active = false;
      unlisten.then((f) => f());
    };
  }, []);

  return channelData;
}

export default useChannelData;
