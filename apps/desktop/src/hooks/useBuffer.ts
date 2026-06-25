import { attachConsole } from "@tauri-apps/plugin-log";
import { useState, useEffect } from "react";
import { createTauRPCProxy } from "@/bindings";

function useBuffer() {
  const [buffer, setBuffer] = useState<number[] | null>(null);

  useEffect(() => {
    const taurpc = createTauRPCProxy();
    let active = true;
    const consoleAttached = attachConsole();

    // Fetch the current buffer directly so the UI reflects state on load even if
    // the startup sync event fires before this listener is attached.
    taurpc.sync
      .sync_buffer()
      .then((b) => {
        if (active) setBuffer(b.buffer);
      })
      .catch(() => {});

    const unlisten = taurpc.sync.event.on((event) => {
      if (event.type !== "bufferSet") return;
      setBuffer(event.buffer);
    });

    return () => {
      active = false;
      unlisten.then((f) => f());
      consoleAttached.then((detach) => detach());
    };
  }, []);

  return buffer;
}

export default useBuffer;
