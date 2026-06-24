import { useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import useLuxState from "@/hooks/useLuxState";
import DeskRow from "./desk-row";

// Fixed row height drives the virtualizer; rows are uniform faders.
const ROW_HEIGHT = 48;

/**
 * The universe desk: every DMX512 channel as a fader, virtualized so 512 rows
 * scroll smoothly while only the ~visible handful mount. Channels 1–6 are the
 * labelled RGBAW fixture (also driven by the color picker); 7–512 are raw.
 */
export default function ControlGrid() {
  const data = useLuxState();
  const parentRef = useRef<HTMLDivElement>(null);

  const virtualizer = useVirtualizer({
    count: data.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 10,
  });

  if (!data.length) {
    return (
      <div className="mt-4 w-full max-w-xl rounded-md border py-10 text-center text-muted-foreground">
        No channels
      </div>
    );
  }

  return (
    <div
      ref={parentRef}
      className="mt-4 h-[68vh] w-full max-w-xl overflow-auto rounded-md border"
    >
      <div
        className="relative w-full"
        style={{ height: `${virtualizer.getTotalSize()}px` }}
      >
        {virtualizer.getVirtualItems().map((item) => {
          const channel = data[item.index];
          return (
            <div
              key={channel.id}
              data-index={item.index}
              className="absolute left-0 top-0 w-full"
              style={{
                height: `${item.size}px`,
                transform: `translateY(${item.start}px)`,
              }}
            >
              <DeskRow channel={channel} />
            </div>
          );
        })}
      </div>
    </div>
  );
}
