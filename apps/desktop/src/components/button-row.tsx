import { Button } from "@/components/ui/button";
import { debug, trace } from "@tauri-apps/plugin-log";
import { createTauRPCProxy } from "@/bindings";
import { toast } from "sonner";

export function setBuffer(buffer: number[]) {
  const taurpc = createTauRPCProxy();
  taurpc.cmd
    .set_buffer(buffer)
    .then(() => {
      // Debug-only: logged at debug level, not surfaced as a toast.
      debug(`buffer set [${buffer}]`);
    })
    .catch((e) => toast.error(String(e)));
}

// A full 512-channel frame at one level overrides the whole universe.
const universe = (level: number) => Array(512).fill(level);

// The two built-in presets. User-defined presets (saved scenes) are the
// planned next residents of this row.
const buttons = [
  {
    children: "⚫️ Blackout",
    onClick: () => setBuffer(universe(0)),
  },
  {
    children: "💡 Full Bright",
    onClick: () => setBuffer(universe(255)),
  },
];

function ControlButton({
  children,
  onClick,
}: {
  children: string;
  onClick: () => void;
}) {
  const handleClick = () => {
    trace(`frontend sending ${children}`);
    onClick();
  };
  return (
    <Button key={children} onClick={handleClick} variant="link" size="sm">
      {children}
    </Button>
  );
}

/** The preset row, shown on both control surfaces (fixtures + universe). */
function ButtonRow() {
  return (
    <div className="flex shrink-0 justify-center gap-2 py-2">
      {buttons.map(ControlButton)}
    </div>
  );
}

export default ButtonRow;
