import { deleteChannel, editChannel } from "./app/actions";

type LuxLabelColor =
  | "Red"
  | "Green"
  | "Blue"
  | "Amber"
  | "White"
  | "Brightness";

export type LuxBuffer = number[] | null;

export type LuxChannel = {
  id: string;
  disabled: boolean;
  channel_number: number;
  label: string;
  label_color: LuxLabelColor;
};

export interface ChannelProps extends LuxChannel {
  value: number;
}

export interface LightColorVariants
  extends VariantProps<typeof lightColorVariants> {}

declare module "@tanstack/table-core" {
  interface TableMeta<TData extends ChannelProps> {
    deleteChannel: typeof deleteChannel;
    editChannel: typeof editChannel;
  }
}
