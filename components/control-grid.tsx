"use client";

import Channel from "@/components/channel";
import { type LuxChannel } from "@/lib/utils";
import { Table, TableBody, TableCaption } from "@/components/ui/table";
import { useChannels } from "@/context-providers/channel-data-provider";
import { useBuffer } from "@/context-providers/buffer-provider";

export default function ControlGrid() {
  const luxChannels = useChannels();
  const buffer = useBuffer();

  const intoControlGrid = (channel: LuxChannel) => ({
    ...channel,
    value: [buffer[channel.channel_number - 1]],
  });
  return (
    <>
      hi
      <Table className=" caption-top">
        <TableBody>{luxChannels.map(intoControlGrid).map(Channel)}</TableBody>
      </Table>
    </>
  );
}
