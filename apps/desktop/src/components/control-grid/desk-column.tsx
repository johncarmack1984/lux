import type { ChannelProps } from "@/global";
import { Slider } from "@/components/ui/slider";
import { Button } from "@/components/ui/button";
import { cn, lightColorVariants } from "@/lib/utils";
import useChannelFader from "./use-channel-fader";

/**
 * One channel of the vertically-oriented desk: the same badge / label / 0-255
 * toggle / slider as DeskRow, stacked into a console-style fader strip.
 * Rendered inside the virtualized grid, so only the visible columns mount.
 */
const DeskColumn = ({ channel }: { channel: ChannelProps }) => {
  const { channelNumber, label, labelColor } = channel;
  const { values, drag, toggle } = useChannelFader(channel);

  return (
    <div className="flex h-full w-full flex-col items-center gap-2 px-1 py-3">
      <div
        className={cn(
          "shrink-0 text-xs tabular-nums",
          lightColorVariants({ labelColor })
        )}
      >
        {channelNumber}
      </div>
      <span className="w-full shrink-0 truncate text-center text-xs text-muted-foreground">
        {label}
      </span>
      <Button
        onClick={toggle}
        variant="outline"
        size="sm"
        className="h-7 w-12 shrink-0 px-0 text-xs tabular-nums"
      >
        {values[0].toString().padStart(3, "0")}
      </Button>
      <Slider
        orientation="vertical"
        aria-label={`Channel ${channelNumber} (${label})`}
        value={values}
        onValueChange={drag}
        max={255}
        step={1}
        className="min-h-0 flex-1"
      />
    </div>
  );
};

export default DeskColumn;
