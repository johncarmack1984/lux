"use client";

import { listen } from "@tauri-apps/api/event";
import { attachConsole, debug, trace } from "@tauri-apps/plugin-log";
import { useState, useEffect, useCallback } from "react";
import { toast } from "sonner";

const detach = async () => await attachConsole();

function useBuffer() {
  const [buffer, setBuffer] = useState<number[] | null>(null);

  const setupListeners = useCallback(async () => {
    await listen<number[]>("buffer_set", ({ payload }) => {
      trace(`useBuffer listen buffer_set [${payload}]`);
      setBuffer(payload);
    }).catch(toast.error);
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
