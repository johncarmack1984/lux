import { useState } from "react";
import { toast } from "sonner";
import { MonitorSmartphone, Settings } from "lucide-react";
import type { SliderOrientation } from "@/bindings";
import useAuth from "@/hooks/useAuth";
import { useSliderOrientation } from "@/hooks/useSettings";
import { setSliderOrientation } from "@/lib/actions";
import { Button } from "@/components/ui/button";
import DevicesDialog from "@/components/devices-dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

/**
 * App settings in the nav — user preferences that persist on this device and
 * sync to the account when signed in (the desk's fader orientation), plus the
 * Devices manager (pair/remove headless lux-node boxes) once signed in.
 */
export default function SettingsMenu() {
  const orientation = useSliderOrientation();
  const status = useAuth();
  const [devicesOpen, setDevicesOpen] = useState(false);

  const onOrientationChange = (value: string) => {
    setSliderOrientation(value as SliderOrientation).catch((e) =>
      toast.error(String(e))
    );
  };

  return (
    <>
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
          {status?.signedIn ? (
            <>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                onSelect={() => setDevicesOpen(true)}
                className="gap-2"
              >
                <MonitorSmartphone className="size-4" /> Devices…
              </DropdownMenuItem>
            </>
          ) : null}
        </DropdownMenuContent>
      </DropdownMenu>

      <DevicesDialog open={devicesOpen} onOpenChange={setDevicesOpen} />
    </>
  );
}
