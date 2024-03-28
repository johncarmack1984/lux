"use client";

import { BufferProvider } from "@/context-providers/buffer-provider";
import { ChannelsProvider } from "@/context-providers/channel-data-provider";

export default function Providers({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <BufferProvider>
      <ChannelsProvider>{children}</ChannelsProvider>
    </BufferProvider>
  );
}
