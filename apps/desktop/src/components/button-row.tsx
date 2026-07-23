import { Button } from "@/components/ui/button";
import { trace } from "@tauri-apps/plugin-log";
import { toast } from "sonner";
import useBuffer from "@/hooks/useBuffer";
import {
  togglePreset,
  useIsPresetActive,
  usePresetReconcile,
} from "@/lib/preset-toggle";

/** A full 512-channel frame at one level overrides the whole universe. */
const universeWrites = (level: number) =>
  new Map(Array.from({ length: 512 }, (_, i) => [i + 1, level] as const));

// The two built-in presets. User-defined presets (saved scenes) are the
// planned next residents of this row.
const presets = [
  {
    id: "blackout",
    children: "⚫️ Blackout",
    writes: () => universeWrites(0),
  },
  {
    id: "full-bright",
    children: "💡 Full Bright",
    writes: () => universeWrites(255),
  },
];

/** Both built-in presets write the whole universe, so they share one lane. */
const SETUP_SCOPE = { kind: "setup" } as const;

/**
 * One preset button. Its own `useIsPresetActive` subscription keeps the pressed
 * state per-preset (and a hook out of the parent's render loop).
 */
function PresetButton({
  id,
  children,
  writes,
  disabled,
}: (typeof presets)[number] & { disabled: boolean }) {
  const active = useIsPresetActive(id);
  return (
    <Button
      variant={active ? "secondary" : "link"}
      size="sm"
      aria-pressed={active}
      disabled={disabled}
      onClick={() => {
        trace(`frontend toggling ${children}`);
        togglePreset(id, writes(), SETUP_SCOPE).catch((e) =>
          toast.error(String(e)),
        );
      }}
    >
      {children}
    </Button>
  );
}

/**
 * The preset row, shown on both control surfaces (fixtures + universe).
 * Presets toggle: engaging one remembers the frame it replaced, pressing it
 * again restores that frame (see lib/preset-toggle).
 */
function ButtonRow() {
  const buffer = useBuffer();
  usePresetReconcile(buffer);
  return (
    <div className="flex shrink-0 justify-center gap-2 py-2">
      {presets.map((preset) => (
        <PresetButton key={preset.id} {...preset} disabled={!buffer} />
      ))}
    </div>
  );
}

export default ButtonRow;
