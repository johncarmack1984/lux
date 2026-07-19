import { Button } from "@/components/ui/button";
import { trace } from "@tauri-apps/plugin-log";
import { toast } from "sonner";
import useBuffer from "@/hooks/useBuffer";
import {
  togglePreset,
  useActivePresetId,
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

/**
 * The preset row, shown on both control surfaces (fixtures + universe).
 * Presets toggle: engaging one remembers the frame it replaced, pressing it
 * again restores that frame (see lib/preset-toggle).
 */
function ButtonRow() {
  const buffer = useBuffer();
  usePresetReconcile(buffer);
  const activeId = useActivePresetId();
  return (
    <div className="flex shrink-0 justify-center gap-2 py-2">
      {presets.map(({ id, children, writes }) => {
        const active = activeId === id;
        return (
          <Button
            key={id}
            variant={active ? "secondary" : "link"}
            size="sm"
            aria-pressed={active}
            disabled={!buffer}
            onClick={() => {
              trace(`frontend toggling ${children}`);
              togglePreset(id, writes()).catch((e) => toast.error(String(e)));
            }}
          >
            {children}
          </Button>
        );
      })}
    </div>
  );
}

export default ButtonRow;
