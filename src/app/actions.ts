import type { LuxChannel } from "@/global";
import { invoke } from "@tauri-apps/api/core";
import { trace } from "@tauri-apps/plugin-log";
import { toast } from "sonner";

const editChannel = async (channelId: string, newMetadata: LuxChannel) => {
  trace(`frontend sending editChannel ${channelId}`);
  return await invoke<LuxChannel>("edit_channel", { channelId, newMetadata });
};

const deleteChannel = async (channelId: string) => {
  trace(`frontend sending deleteChannel ${channelId}`);
  return await invoke<void>("delete_channel", { channelId });
};

async function setChannelValue({
  channelNumber,
  value,
}: {
  channelNumber: number;
  value: number;
}) {
  return await invoke("update_channel_value", {
    channelNumber,
    value,
  }).catch(toast.error);
}

async function setChannelMetadata({
  channelId,
  newMetadata,
}: {
  channelId: string;
  newMetadata: LuxChannel;
}) {
  return await invoke("update_channel_metadata", {
    channelId,
    newMetadata,
  }).catch(toast.error);
}

export { editChannel, deleteChannel, setChannelValue, setChannelMetadata };
