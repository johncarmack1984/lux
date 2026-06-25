import { useEffect, useMemo } from "react";

import useBuffer from "@/hooks/useBuffer";
import useChannelsMetadata from "@/hooks/useChannelsMetaData";
import { createTauRPCProxy } from "@/bindings";

const useLuxState = () => {
  useEffect(() => {
    const taurpc = createTauRPCProxy();
    taurpc.cmd.sync_state();
  }, []);

  const luxChannels = useChannelsMetadata();
  const buffer = useBuffer();

  return useMemo(() => {
    if (!luxChannels) return [];
    if (!buffer) return [];
    return luxChannels.map((channel) => ({
      ...channel,
      value: buffer[channel.channelNumber - 1] ?? 0,
    }));
  }, [buffer, luxChannels]);
};

export default useLuxState;
