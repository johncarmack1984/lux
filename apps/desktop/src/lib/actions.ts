import { createTauRPCProxy } from "@/bindings";

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

export { setChannelValue };
