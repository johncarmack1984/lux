import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "./cn";

/* The classic shadcn split: `buttonVariants` is exported on its own so links
   can dress as buttons (<a className={buttonVariants(...)}>) without nesting
   interactive elements. */

export const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-[var(--radius)] font-semibold leading-tight transition-[transform,background-color,border-color] duration-150 active:translate-y-px disabled:pointer-events-none disabled:opacity-50",
  {
    variants: {
      variant: {
        primary:
          "bg-[var(--primary)] text-[var(--primary-foreground)] hover:bg-[#f0b155]",
        ghost:
          "border border-[var(--border)] text-[var(--foreground)] hover:border-[var(--muted-foreground)]",
      },
      size: {
        default: "px-5 py-3 text-base",
        sm: "px-3.5 py-2 text-sm",
      },
    },
    defaultVariants: {
      variant: "primary",
      size: "default",
    },
  },
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, ...props }, ref) => (
    <button
      ref={ref}
      className={cn(buttonVariants({ variant, size }), className)}
      {...props}
    />
  ),
);
Button.displayName = "Button";
