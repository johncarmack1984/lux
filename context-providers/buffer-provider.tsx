"use client";

import { LuxBuffer } from "@/lib/utils";
import { listen } from "@tauri-apps/api/event";
import { createContext, useContext, useEffect, useState } from "react";
// import { debug } from "tauri-plugin-log-api";

const BufferContext = createContext<LuxBuffer>([]);

function useBuffer() {
  const [buffer, setBuffer] = useState<LuxBuffer>(useContext(BufferContext));

  useEffect(() => {
    const unlisten = async () => {
      listen<LuxBuffer>("buffer_update", ({ payload }) => {
        // debug(`buffer_update: ${payload}`);
        setBuffer(payload);
      });
    };
    return () => {
      unlisten();
    };
  }, []);
  return buffer;
}

function BufferProvider({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  const buffer = useBuffer();
  return (
    <BufferContext.Provider value={buffer}>{children}</BufferContext.Provider>
  );
}

export { BufferProvider, useBuffer };
