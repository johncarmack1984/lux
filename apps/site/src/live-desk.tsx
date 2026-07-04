import { useState, type CSSProperties } from "react";
import { ChannelRow } from "@lux/ui";

/* The hero demo: the app's default fixture card, functional. Same six
   channels, same additive RGBAW mix the desktop ships, with a strip of
   "room" above so you can see what the levels do. */

type Levels = { r: number; g: number; b: number; a: number; w: number; d: number };

const CHANNELS = [
  { key: "r", label: "Red", color: "var(--lux-ch-r)" },
  { key: "g", label: "Green", color: "var(--lux-ch-g)" },
  { key: "b", label: "Blue", color: "var(--lux-ch-b)" },
  { key: "a", label: "Amber", color: "var(--lux-ch-a)" },
  { key: "w", label: "White", color: "var(--lux-ch-w)" },
  { key: "d", label: "Dimmer", color: "var(--lux-ch-dim)" },
] as const;

function mix({ r, g, b, a, w, d }: Levels) {
  const [rn, gn, bn, an, wn, dn] = [r, g, b, a, w, d].map((v) => v / 255);
  const mr = Math.min(1, rn + 0.98 * an + wn);
  const mg = Math.min(1, gn + 0.62 * an + wn);
  const mb = Math.min(1, bn + 0.1 * an + wn);
  const level = dn * Math.max(mr, mg, mb);
  const color = `rgb(${Math.round(mr * 255)} ${Math.round(mg * 255)} ${Math.round(mb * 255)})`;
  return { color, level };
}

export function LiveDesk() {
  const [levels, setLevels] = useState<Levels>({ r: 0, g: 0, b: 0, a: 255, w: 0, d: 170 });
  const { color, level } = mix(levels);

  return (
    <div>
      <div
        className="stage"
        style={{ "--mix": color, "--mix-level": String(0.15 + 0.8 * level) } as CSSProperties}
      >
        <div className="beam" />
        <div className="lamp" />
      </div>

      <div className="mt-4 rounded-(--radius) border bg-surface p-5">
        <div className="flex items-start justify-between">
          <div>
            <h3 className="text-lg font-semibold">Default Fixture</h3>
            <p className="text-sm text-mut">Channels 1 - 6</p>
          </div>
          <span className="mono chip">UNIVERSE 1</span>
        </div>

        <div className="mt-4 flex items-center gap-3">
          <span className="text-[15px] font-medium">Color</span>
          <span
            aria-hidden
            className="size-7 rounded-full border"
            style={{ background: color, opacity: 0.25 + 0.75 * level }}
          />
        </div>

        <div className="mt-4 space-y-3">
          {CHANNELS.map((ch, i) => (
            <ChannelRow
              key={ch.key}
              channel={i + 1}
              label={ch.label}
              color={ch.color}
              value={levels[ch.key]}
              onValueChange={(value) => setLevels((prev) => ({ ...prev, [ch.key]: value }))}
            />
          ))}
        </div>
      </div>

      <p className="mt-3.5 text-center text-sm text-mut">
        Go ahead, it's live. Same mixing math as the app.
      </p>
    </div>
  );
}
