import { attachConsole, trace } from "@tauri-apps/plugin-log";
import { useState, useEffect, useCallback } from "react";
import { toast } from "sonner";
import { createTauRPCProxy } from "@/bindings";

const detach = async () => await attachConsole();

function useBuffer() {
  const [buffer, setBuffer] = useState<number[] | null>(null);

  const setupListeners = useCallback(async () => {
    const taurpc = createTauRPCProxy();
    await taurpc.sync.event
      .on((event) => {
        if (event.type !== "bufferSet") return;
        trace(`useBuffer listen buffer_set [${event.buffer}]`);
        setBuffer(event.buffer);
      })
      .catch((e) => toast.error(String(e)));
  }, []);

  useEffect(() => {
    setupListeners();
    return () => {
      detach();
    };
  }, [setupListeners]);

  return buffer;
}

export default useBuffer;
