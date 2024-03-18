import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export type ChannelType = {
  value: number;
  label: string;
  channel: number;
};

export const channels: ChannelType[] = [
  { value: 0, label: "Red", channel: 1 },
  { value: 0, label: "Green", channel: 2 },
  { value: 0, label: "Blue", channel: 3 },
  { value: 0, label: "Amber", channel: 4 },
  { value: 0, label: "White", channel: 5 },
  { value: 0, label: "Brightness", channel: 6 },
];

export const lightColors = {
  red: "bg-red-500",
  green: "bg-green-500",
  blue: "bg-blue-500",
  amber: "bg-amber-200",
  white: "bg-white",
};
