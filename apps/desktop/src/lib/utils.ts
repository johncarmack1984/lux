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
    "border",
    "flex",
    "items-center",
    "justify-center",
    "rounded-full",
  ],
  {
    variants: {
      labelColor: {
        Red: "bg-red-500 text-white",
        Green: "bg-green-500",
        Blue: "bg-blue-500",
        Amber: "bg-amber-200 text-amber-800",
        White: "bg-white text-black",
        Brightness: "bg-black text-white",
        Generic: "bg-muted text-muted-foreground",
      },
    },
  }
);

export { cn, lightColorVariants };
