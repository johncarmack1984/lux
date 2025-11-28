"use client";

import { attachConsole, debug, trace } from "@tauri-apps/plugin-log";
import { useState, useEffect, useCallback } from "react";
import { toast } from "sonner";
import { createTauRPCProxy } from "@/bindings";

const detach = async () => await attachConsole();

function useBuffer() {
  const [buffer, setBuffer] = useState<number[] | null>(null);

  const setupListeners = useCallback(async () => {
    const taurpc = createTauRPCProxy();
    await taurpc.sync.buffer_set
      .on((buffer) => {
        trace(`useBuffer listen buffer_set [${buffer}]`);
        setBuffer(buffer);
      })
      .catch(toast.error);
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
