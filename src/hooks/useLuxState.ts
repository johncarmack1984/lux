"use client";

import { useEffect, useMemo, useReducer } from "react";

import useBuffer from "@/hooks/useBuffer";
import useChannelsMetadata from "@/hooks/useChannelsMetaData";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import type { LuxBuffer } from "@/global";
import { trace } from "@tauri-apps/plugin-log";

const useLuxState = () => {
  useEffect(() => {
    trace("frontend invoking sync_state");
    invoke<LuxBuffer | string>("sync_state")
      .then(toast.success)
      .catch(toast.error)
      .finally(() => trace("frontend invoked sync_state"));
  }, []);

  const luxChannels = useChannelsMetadata();
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
