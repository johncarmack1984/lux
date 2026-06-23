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
