"use client";

import { LuxChannel } from "@/lib/utils";
// import { invoke } from "@tauri-apps/api";
import { listen, once } from "@tauri-apps/api/event";
import { createContext, useContext, useEffect, useState } from "react";
import { debug } from "@tauri-apps/plugin-log";

const ChannelsContext = createContext<LuxChannel[]>([]);

function useChannels() {
  const [channels, setChannels] = useState<LuxChannel[]>(
    useContext(ChannelsContext)
  );
  useEffect(() => {
    // debug(`usechannels root: ${Object.keys(channels)}`);
    // once<LuxChannel[]>("loaded", (event) => {
    // debug(`loaded event: ${event}`);
    // invoke<LuxChannel[]>("get_channel_data").then((channels) => {
    //     // debug(`channel provider invoke: ${channels}`);
    //     // debug(`channels: ${Object.keys(payload).length}`);
    // setChannels(channels);
    // });
    // });
    const unlisten = async () => {
      //   listen<LuxChannel[]>("channel_data_update", ({ payload }) => {
      //     debug(`channel provider listen: ${Object.keys(payload).length}`);
      //     setChannels(payload);
      //   });
    };
    return () => {
      unlisten();
    };
  }, []);
  return channels;
}

const ChannelsProvider = ({ children }: { children: React.ReactNode }) => {
  const channels = useChannels();
  return (
    <ChannelsContext.Provider value={channels}>
      {children}
    </ChannelsContext.Provider>
  );
};

export { ChannelsProvider, useChannels };
