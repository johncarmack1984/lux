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

export type LuxBuffer = number[];

export type LuxChannel = {
  disabled: boolean;
  channel_number: number;
  label: string;
  label_color: LuxLabelColor;
};
