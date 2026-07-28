import { useEffect, useState } from "react";
import { RgbaColorPicker, type RgbaColor } from "react-colorful";
import { type Fixture, type LuxLabelColor } from "@/bindings";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent } from "@/components/ui/popover";
import ColorTrigger from "@/components/color-picker/color-trigger";
import useThrottle from "@/hooks/useThrottle";
import { setChannelValue } from "@/lib/actions";
import { emittersToRgb, mixToEmitters } from "@/lib/color-mix";
import { togglePreset, useIsPresetActive } from "@/lib/preset-toggle";

const FIXTURE_PRESETS: ReadonlyArray<{
  id: string;
  label: string;
  color: RgbaColor;
}> = [
  { id: "reading-light", label: "Reading Light", color: { r: 255, g: 128, b: 0, a: 0.4 } },
  { id: "daylight", label: "Daylight", color: { r: 255, g: 255, b: 255, a: 0.4 } },
  { id: "fire", label: "Fire", color: { r: 255, g: 80, b: 0, a: 0.4 } },
  { id: "rose", label: "Rose", color: { r: 255, g: 80, b: 120, a: 0.4 } },
  { id: "lavender", label: "Lavender", color: { r: 150, g: 120, b: 255, a: 0.4 } },
  { id: "cobalt", label: "Cobalt", color: { r: 0, g: 70, b: 200, a: 0.4 } },
  { id: "steel", label: "Steel", color: { r: 140, g: 160, b: 190, a: 0.4 } },
  { id: "dusk", label: "Dusk", color: { r: 190, g: 155, b: 140, a: 0.4 } },
];

/** First DMX address (1-based) within the fixture carrying `role`, or null. */
function roleAddress(fixture: Fixture, role: LuxLabelColor): number | null {
  const i = fixture.channels.findIndex((c) => c.role === role);
  return i < 0 ? null : fixture.address + i;
}

function FixturePresetButton({
  preset,
  fixture,
  disabled,
}: {
  preset: (typeof FIXTURE_PRESETS)[number];
  fixture: Fixture;
  disabled: boolean;
}) {
  const presetId = `${preset.id}-${fixture.id}`;
  const active = useIsPresetActive(presetId);

  const onClick = () => {
    const r = roleAddress(fixture, "Red");
    const g = roleAddress(fixture, "Green");
    const b = roleAddress(fixture, "Blue");
    const amber = roleAddress(fixture, "Amber");
    const white = roleAddress(fixture, "White");
    const dimmer = roleAddress(fixture, "Brightness");
    const mix = mixToEmitters(preset.color.r, preset.color.g, preset.color.b, {
      amber: amber !== null,
      white: white !== null,
    });
    const writes = new Map<number, number>();
    for (const [addr, value] of [
      [r, mix.r],
      [g, mix.g],
      [b, mix.b],
      [amber, mix.a],
      [white, mix.w],
      [dimmer, Math.round(preset.color.a * 255)],
    ] as Array<[number | null, number]>) {
      if (addr !== null) writes.set(addr, value);
    }
    togglePreset(presetId, writes, {
      kind: "fixture",
      fixtureId: fixture.id,
    }).catch(() => {});
  };

  return (
    <Button
      variant={active ? "default" : "outline"}
      size="sm"
      className="h-7 gap-1.5 px-2 text-xs"
      aria-pressed={active}
      disabled={disabled}
      onClick={onClick}
    >
      <span
        className="size-2.5 shrink-0 rounded-full border border-foreground/20"
        style={{
          backgroundColor: `rgb(${preset.color.r}, ${preset.color.g}, ${preset.color.b})`,
        }}
      />
      {preset.label}
    </Button>
  );
}

/**
 * Color control for a fixture with R/G/B roles. The wheel decomposes the picked
 * color across whatever emitters the fixture has — White takes the achromatic
 * part, Amber the warm part, R/G/B the rest (see lib/color-mix) — each written
 * to its own address. The swatch recombines them, so it stays honest after a mix
 * or a manual amber/white nudge. Dimmer (alpha) is the master level.
 */
export default function FixtureColor({
  fixture,
  buffer,
  label,
}: {
  fixture: Fixture;
  buffer: number[] | null;
  /** Trigger label; pass "" for a bare swatch (collapsed cards). */
  label?: string;
}) {
  const r = roleAddress(fixture, "Red");
  const g = roleAddress(fixture, "Green");
  const b = roleAddress(fixture, "Blue");
  const amber = roleAddress(fixture, "Amber");
  const white = roleAddress(fixture, "White");
  const dimmer = roleAddress(fixture, "Brightness");

  const [color, setColor] = useState<RgbaColor>({ r: 0, g: 0, b: 0, a: 1 });

  useEffect(() => {
    const at = (addr: number | null) =>
      addr && buffer ? buffer[addr - 1] ?? 0 : 0;
    const rgb = emittersToRgb({
      r: at(r),
      g: at(g),
      b: at(b),
      a: at(amber),
      w: at(white),
    });
    setColor({ ...rgb, a: dimmer ? at(dimmer) / 255 : 1 });
  }, [buffer, r, g, b, amber, white, dimmer]);

  /** The per-address writes that render `next` on this fixture's emitters. */
  const emitterWrites = (next: RgbaColor): Array<[number | null, number]> => {
    const mix = mixToEmitters(next.r, next.g, next.b, {
      amber: amber !== null,
      white: white !== null,
    });
    return [
      [r, mix.r],
      [g, mix.g],
      [b, mix.b],
      [amber, mix.a],
      [white, mix.w],
      [dimmer, Math.round(next.a * 255)],
    ];
  };

  const send = useThrottle((next: RgbaColor) => {
    for (const [addr, value] of emitterWrites(next)) {
      if (addr) setChannelValue({ channelNumber: addr, value }).catch(() => {});
    }
  }, 40);

  const onChange = (next: RgbaColor) => {
    setColor(next);
    send(next);
  };

  // Glow tracks the dimmer when present, else the brightest color channel.
  const luminance = dimmer ? color.a : Math.max(color.r, color.g, color.b) / 255;

  return (
    <Popover>
      {/* -ml-2 lines the labeled trigger's text up with card content; the
          bare swatch has no padding to compensate for. */}
      <ColorTrigger
        color={color}
        luminance={luminance}
        label={label}
        className={label === "" ? undefined : "-ml-2"}
      />
      <PopoverContent align="start">
        <RgbaColorPicker className="mx-auto" color={color} onChange={onChange} />
        <div className="mt-3 grid grid-cols-2 gap-1.5">
          {FIXTURE_PRESETS.map((preset) => (
            <FixturePresetButton
              key={preset.id}
              preset={preset}
              fixture={fixture}
              disabled={!buffer}
            />
          ))}
        </div>
      </PopoverContent>
    </Popover>
  );
}
