import { deleteChannel, editChannel } from "./app/actions";
import {
  createTauRPCProxy,
  type LuxLabelColor,
  type LuxBuffer,
  type LuxChannel,
} from "@/bindings";

export { LuxChannel, LuxBuffer, LuxLabelColor };

export interface ChannelProps extends LuxChannel {
  value: number;
}

export interface LightColorVariants extends VariantProps<
  typeof lightColorVariants
> {}

declare module "@tanstack/table-core" {
  interface TableMeta<TData extends ChannelProps> {
    deleteChannel: typeof deleteChannel;
    editChannel: typeof editChannel;
  }
}
