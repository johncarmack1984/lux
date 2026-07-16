import { toast } from "sonner";
import { Settings } from "lucide-react";
import type { SliderOrientation } from "@/bindings";
import { useSliderOrientation } from "@/hooks/useSettings";
import { setSliderOrientation } from "@/lib/actions";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

/**
 * App settings in the nav — user preferences that persist on this device and
 * sync to the account when signed in. Currently: the desk's fader orientation.
 */
export default function SettingsMenu() {
  const orientation = useSliderOrientation();

  const onOrientationChange = (value: string) => {
    setSliderOrientation(value as SliderOrientation).catch((e) =>
      toast.error(String(e))
    );
  };

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size="sm"
          className="px-2 text-muted-foreground hover:text-foreground"
          aria-label="Settings"
        >
          <Settings className="size-4" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-48">
        <DropdownMenuLabel className="font-normal text-muted-foreground">
          Slider orientation
        </DropdownMenuLabel>
        <DropdownMenuRadioGroup
          value={orientation}
          onValueChange={onOrientationChange}
        >
          <DropdownMenuRadioItem value="vertical">
            Vertical
          </DropdownMenuRadioItem>
          <DropdownMenuRadioItem value="horizontal">
            Horizontal
          </DropdownMenuRadioItem>
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
