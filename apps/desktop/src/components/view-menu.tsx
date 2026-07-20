import { Link, useLocation } from "@tanstack/react-router";
import { Check, ChevronsUpDown, Lightbulb, SlidersVertical, Users } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

const VIEWS = [
  { to: "/", label: "Fixtures", icon: Lightbulb },
  { to: "/universe", label: "Universe", icon: SlidersVertical },
  { to: "/shared", label: "Shared with you", icon: Users },
] as const;

/**
 * The nav's view picker: which control surface is showing — Fixtures (the
 * patched cards) or Universe (the raw 512-fader desk). One dropdown instead of
 * two nav links keeps the bar breathing at phone widths.
 */
export default function ViewMenu() {
  const pathname = useLocation({ select: (location) => location.pathname });
  const active =
    VIEWS.find((view) =>
      view.to === "/" ? pathname === "/" : pathname.startsWith(view.to)
    ) ?? VIEWS[0];

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="outline" size="sm" className="gap-1.5">
          <active.icon className="size-3.5" />
          <span>{active.label}</span>
          <ChevronsUpDown className="size-3.5 text-muted-foreground" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start">
        {VIEWS.map((view) => (
          <DropdownMenuItem key={view.to} asChild>
            <Link to={view.to} className="flex w-full items-center gap-2">
              <view.icon className="size-4 text-muted-foreground" />
              {view.label}
              {view.to === active.to && <Check className="ml-auto size-4" />}
            </Link>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
