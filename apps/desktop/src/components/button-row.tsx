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

// A full 512-channel frame at one level overrides the whole universe; a 6-byte
// fixture write (Default) only overlays the leading RGBAW slots.
const universe = (level: number) => Array(512).fill(level);

const buttons = [
  {
    children: "⚫️ Blackout",
    onClick: () => setBuffer(universe(0)),
  },
  {
    children: "✅ Default",
    onClick: () => setBuffer([121, 255, 255, 0, 0, 42]),
  },
  {
    children: "💡 Full Bright",
    onClick: () => setBuffer(universe(255)),
  },
  // Planned feature
  // { children: "🌈 RGB Chase",
  //   onClick: () => invoke("rgb_chase")
  // },
  // Debug functions
  // {
  //   children: "🔄 Sync",
  //   onClick: () => invoke("sync_state"),
  // },
  // {
  //   children: "🤘 Get state from Turso",
  //   onClick: async () => await getInitialState(),
  // },
  // {
  //   children: "🚮 Delete all Channels",
  //   onClick: () => invoke("delete_channels"),
  // },
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
    <Button
      key={children}
      onClick={handleClick}
      className=""
      variant="link"
      size="sm"
    >
      {children}
    </Button>
  );
}

function ButtonRow() {
  return (
    <div className="grid-cols-1 sm:grid-cols-3 py-8 grid">
      {buttons.map(ControlButton)}
    </div>
  );
}

export default ButtonRow;
