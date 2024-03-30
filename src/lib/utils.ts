import { cva } from "class-variance-authority";
import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

const lightColorVariants = cva(
  [
    "size-8",
    "border-border",
    "border-[1px]",
    "flex",
    "items-center",
    "justify-center",
    "rounded-full",
  ],
  {
    variants: {
      label_color: {
        Red: "bg-red-500",
        Green: "bg-green-500",
        Blue: "bg-blue-500",
        Amber: "bg-amber-200 text-amber-800",
        White: "bg-white text-black",
        Brightness: "bg-black text-white",
      },
    },
  }
);

export { cn, lightColorVariants };
