import type { LuxChannel } from "@/global";
import { trace } from "@tauri-apps/plugin-log";
import { createTauRPCProxy } from "@/bindings";

const editChannel = async (channelId: number, newMetadata: LuxChannel) => {
  trace(`frontend sending editChannel ${channelId}`);
  const taurpc = createTauRPCProxy();
  return await taurpc.cmd.update_channel_metadata(channelId, newMetadata);
};

const deleteChannel = async (channelId: number) => {
  trace(`frontend sending deleteChannel ${channelId}`);
  const taurpc = createTauRPCProxy();
  return await taurpc.cmd.delete_channel(channelId);
};

async function setChannelValue({
  channelNumber,
  value,
}: {
  channelNumber: number;
  value: number;
}) {
  const taurpc = createTauRPCProxy();
  return await taurpc.cmd.update_channel_value(channelNumber, value);
}

async function setChannelMetadata({
  channelId,
  newMetadata,
}: {
  channelId: number;
  newMetadata: LuxChannel;
}) {
  const taurpc = createTauRPCProxy();
  return await taurpc.cmd.update_channel_metadata(channelId, newMetadata);
}

export { editChannel, deleteChannel, setChannelValue, setChannelMetadata };
