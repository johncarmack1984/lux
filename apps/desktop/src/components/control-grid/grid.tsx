import { useMemo, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import useLuxState from "@/hooks/useLuxState";
import useFixtures from "@/hooks/useFixtures";
import useSettings from "@/hooks/useSettings";
import { cn } from "@/lib/utils";
import { patchChannelMeta } from "@/lib/channel-map";
import DeskRow from "./desk-row";
import DeskColumn from "./desk-column";

// Fixed lane sizes drive the virtualizer; rows and columns are uniform faders.
const ROW_HEIGHT = 48;
const COLUMN_WIDTH = 64;

type Channels = ReturnType<typeof useLuxState>;

/**
 * The universe desk: every DMX512 channel as a fader, virtualized so 512 lanes
 * scroll smoothly while only the ~visible handful mount. Channels covered by
 * the active setup's patch carry that fixture's role color and label; every
 * other channel is a plain numbered fader — an empty setup is 512 plain
 * sliders.
 *
 * The "slider orientation" user setting picks the layout: vertical faders in a
 * horizontally-scrolling desk (the default, like a lighting console), or
 * horizontal faders in a vertically-scrolling list. The desk waits for the
 * settings read — rendering the default while it's in flight would flash the
 * wrong layout (and remount the virtualizer) for horizontal users on every
 * launch.
 */
export default function ControlGrid() {
  const channels = useLuxState();
  const fixtures = useFixtures();
  const settings = useSettings();

  // Patch-derived labels only: unpatched channels render blank + neutral.
  const data = useMemo(() => {
    const meta = patchChannelMeta(fixtures);
    return channels.map((channel) => {
      const patched = meta.get(channel.channelNumber);
      return patched
        ? { ...channel, label: patched.label, labelColor: patched.labelColor }
        : { ...channel, label: "", labelColor: "Generic" as const };
    });
  }, [channels, fixtures]);

  if (!data.length || settings === null) {
    return (
      <div className="mx-auto mt-4 w-full max-w-xl rounded-md border py-10 text-center text-muted-foreground">
        {/* Blank while queries settle; "No channels" only once we know. */}
        {settings === null ? " " : "No channels"}
      </div>
    );
  }

  const vertical = (settings.sliderOrientation ?? "vertical") === "vertical";
  // key: flipping the orientation remounts the desk, giving the virtualizer a
  // fresh instance instead of mutating one across axes.
  return (
    <Desk
      key={vertical ? "vertical" : "horizontal"}
      data={data}
      vertical={vertical}
    />
  );
}

/** One virtualized desk along either axis; see ControlGrid for the layouts. */
function Desk({ data, vertical }: { data: Channels; vertical: boolean }) {
  const parentRef = useRef<HTMLDivElement>(null);

  const virtualizer = useVirtualizer({
    count: data.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => (vertical ? COLUMN_WIDTH : ROW_HEIGHT),
    overscan: 10,
    horizontal: vertical,
  });
  const total = `${virtualizer.getTotalSize()}px`;

  return (
    <div
      ref={parentRef}
      className={cn(
        // Fill whatever the route hands us (it fits the desk to the viewport)
        // instead of a hardcoded viewport fraction that overflowed under the
        // presets row.
        "h-full w-full overflow-auto rounded-md border",
        // Horizontal rows read badly at full width; columns want all of it.
        !vertical && "mx-auto max-w-xl"
      )}
    >
      <div
        className={cn("relative", vertical ? "h-full" : "w-full")}
        style={vertical ? { width: total } : { height: total }}
      >
        {virtualizer.getVirtualItems().map((item) => {
          const channel = data[item.index];
          return (
            <div
              key={channel.id}
              data-index={item.index}
              className={cn(
                "absolute left-0 top-0",
                vertical ? "h-full" : "w-full"
              )}
              style={
                vertical
                  ? {
                      width: `${item.size}px`,
                      transform: `translateX(${item.start}px)`,
                    }
                  : {
                      height: `${item.size}px`,
                      transform: `translateY(${item.start}px)`,
                    }
              }
            >
              {vertical ? (
                <DeskColumn channel={channel} />
              ) : (
                <DeskRow channel={channel} />
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
