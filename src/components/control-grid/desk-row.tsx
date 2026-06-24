import { useEffect, useState } from "react";
import { toast } from "sonner";
import type { ChannelProps } from "@/global";
import { Slider } from "@/components/ui/slider";
import { Button } from "@/components/ui/button";
import { cn, lightColorVariants } from "@/lib/utils";
import { setChannelValue } from "@/lib/actions";
import useThrottle from "@/hooks/useThrottle";

/**
 * One channel of the universe desk: a colored channel-number badge, the label,
 * a 0/255 toggle, and the value slider. Rendered inside the virtualized grid, so
 * only the visible rows mount.
 */
const DeskRow = ({ channel }: { channel: ChannelProps }) => {
  const { channelNumber, label, labelColor, value } = channel;
  const [values, setValues] = useState([value]);

  // Drags/clicks fire continuously; keep the UI immediate but throttle the
  // hardware/IPC write so we don't flood the DMX render path.
  const sendValue = useThrottle((next: number) => {
    setChannelValue({ channelNumber, value: next }).catch((e) =>
      toast.error(String(e))
    );
  }, 40);

  // Re-sync when the value changes from elsewhere (color picker, remote, or the
  // optimistic echo of our own write); harmless mid-drag since that echo equals
  // what we just sent.
  useEffect(() => {
    setValues([value]);
  }, [value]);

  const drag = (next: number[]) => {
    setValues(next);
    sendValue(next[0]);
  };

  const toggle = () => {
    const next = values[0] === 0 ? 255 : 0;
    setValues([next]);
    sendValue(next);
  };

  return (
    <div className="flex h-full w-full items-center gap-3 px-3">
      <div
        className={cn(
          "shrink-0 text-xs tabular-nums",
          lightColorVariants({ labelColor })
        )}
      >
        {channelNumber}
      </div>
      <span className="w-20 shrink-0 truncate text-right text-sm text-muted-foreground">
        {label}
      </span>
      <Button
        onClick={toggle}
        variant="outline"
        size="sm"
        className="w-14 shrink-0 tabular-nums"
      >
        {values[0].toString().padStart(3, "0")}
      </Button>
      <Slider
        aria-label={`Channel ${channelNumber} (${label})`}
        value={values}
        onValueChange={drag}
        max={255}
        step={1}
        className="flex-1"
      />
    </div>
  );
};

export default DeskRow;
