import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export type LuxLabelColor =
  | "Red"
  | "Green"
  | "Blue"
  | "Amber"
  | "White"
  | "Brightness";

export type LuxChannel = {
  disabled: boolean;
  value: number;
  label: string;
  label_color: LuxLabelColor;
  channel_number: number;
};

export const channels: LuxChannel[] = [
  {
    disabled: false,
    channel_number: 1,
    label: "Red",
    label_color: "Red",
    value: 121,
  },
  {
    disabled: false,
    channel_number: 2,
    label: "Green",
    label_color: "Green",
    value: 255,
  },
  {
    disabled: false,
    channel_number: 3,
    label: "Blue",
    label_color: "Blue",
    value: 255,
  },
  {
    disabled: false,
    channel_number: 4,
    label: "Amber",
    label_color: "Amber",
    value: 0,
  },
  {
    disabled: false,
    channel_number: 5,
    label: "White",
    label_color: "White",
    value: 0,
  },
  {
    disabled: false,
    channel_number: 6,
    label: "Brightness",
    label_color: "Brightness",
    value: 21,
  },
];
