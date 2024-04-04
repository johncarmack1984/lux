"use client";

import { useEffect, useMemo, useReducer } from "react";

import useBuffer from "@/hooks/useBuffer";
import useChannelData from "@/hooks/useChannelData";
import { invoke } from "@tauri-apps/api/core";
import useTauRPC from "./useTauRPC";

const useLuxState = () => {
  const taurpc = useTauRPC();

  useEffect(() => {
    taurpc?.sync_state();
  }, [taurpc]);

  const luxChannels = useChannelData();
  const buffer = useBuffer();

  return useMemo(() => {
    if (!luxChannels) return [];
    if (!buffer) return [];
    return luxChannels?.map((channel) => ({
      ...channel,
      value: buffer?.[channel.channel_number - 1],
    }));
  }, [buffer, luxChannels]);
};

export default useLuxState;
