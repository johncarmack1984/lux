type LuxLabelColor =
  | "Red"
  | "Green"
  | "Blue"
  | "Amber"
  | "White"
  | "Brightness";

export type LuxBuffer = number[] | null;

export type LuxChannel = {
  disabled: boolean;
  channel_number: number;
  label: string;
  label_color: LuxLabelColor;
};

export interface ChannelProps extends LuxChannel {
  value: number;
}
