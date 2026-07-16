import { useEffect, useState } from "react";
import { RgbaColorPicker, type RgbaColor } from "react-colorful";
import { LampDesk } from "lucide-react";
import { type Fixture, type LuxLabelColor } from "@/bindings";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent } from "@/components/ui/popover";
import ColorTrigger from "@/components/color-picker/color-trigger";
import useThrottle from "@/hooks/useThrottle";
import { setChannelValue } from "@/lib/actions";
import { emittersToRgb, mixToEmitters } from "@/lib/color-mix";

/**
 * Reading light: tungsten/amber at full, master at 40%. Expressed as a picker
 * color so it flows through the same role-aware mix as the wheel — on an
 * amber-equipped fixture the warm content lands on the Amber emitter (≈255),
 * an RGB-only fixture renders the same look with its own emitters, and the
 * alpha drives the Brightness channel where one exists.
 */
const READING_LIGHT: RgbaColor = { r: 255, g: 128, b: 0, a: 0.4 };

/** First DMX address (1-based) within the fixture carrying `role`, or null. */
function roleAddress(fixture: Fixture, role: LuxLabelColor): number | null {
  const i = fixture.channels.findIndex((c) => c.role === role);
  return i < 0 ? null : fixture.address + i;
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

  const send = useThrottle((next: RgbaColor) => {
    const mix = mixToEmitters(next.r, next.g, next.b, {
      amber: amber !== null,
      white: white !== null,
    });
    const writes: Array<[number | null, number]> = [
      [r, mix.r],
      [g, mix.g],
      [b, mix.b],
      [amber, mix.a],
      [white, mix.w],
      [dimmer, Math.round(next.a * 255)],
    ];
    for (const [addr, value] of writes) {
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
      <ColorTrigger
        color={color}
        luminance={luminance}
        label={label}
        className="-ml-2"
      />
      <PopoverContent align="start">
        <RgbaColorPicker className="mx-auto" color={color} onChange={onChange} />
        <div className="mt-3">
          <Button
            variant="outline"
            size="sm"
            className="w-full gap-2"
            onClick={() => onChange(READING_LIGHT)}
          >
            <LampDesk className="size-4" /> Reading light
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  );
}
