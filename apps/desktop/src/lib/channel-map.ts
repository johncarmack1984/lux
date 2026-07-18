import type { Fixture, LuxLabelColor } from "@/bindings";

/**
 * Per-channel display metadata derived from the active setup's patch: a
 * channel carries a role color and label only when a patched fixture occupies
 * it. Everything else is a plain numbered fader — an empty setup's universe is
 * just 512 plain sliders.
 */
export function patchChannelMeta(
  fixtures: Fixture[] | null
): Map<number, { label: string; labelColor: LuxLabelColor }> {
  const meta = new Map<number, { label: string; labelColor: LuxLabelColor }>();
  for (const fixture of fixtures ?? []) {
    fixture.channels.forEach((channel, i) => {
      meta.set(fixture.address + i, {
        label: channel.label,
        labelColor: channel.role,
      });
    });
  }
  return meta;
}
