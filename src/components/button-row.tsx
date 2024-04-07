"use client";

import { Button } from "@/components/ui/button";
import { invoke } from "@tauri-apps/api/core";
import { error, trace } from "@tauri-apps/plugin-log";
import { toast } from "sonner";

function setBuffer(buffer: number[]) {
  invoke("set_buffer", { buffer })
    .then((res) => toast.info(JSON.stringify(res)))
    .catch(toast.error);
}

async function getInitialState() {
  return await invoke("get_initial_state")
    .then((state) => {
      trace(`frontend received ${JSON.stringify(state)}`);
      return state;
    })
    .catch(error);
}

const buttons = [
  {
    children: "⚫️ Blackout",
    onClick: () => setBuffer([0, 0, 0, 0, 0, 0]),
  },
  {
    children: "✅ Default",
    onClick: () => setBuffer([121, 255, 255, 0, 0, 42]),
  },
  {
    children: "💡 Full Bright",
    onClick: () => setBuffer([255, 255, 255, 255, 255, 255]),
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
