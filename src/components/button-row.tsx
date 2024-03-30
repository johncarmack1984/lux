"use client";

import { Button } from "@/components/ui/button";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { debug, trace } from "@tauri-apps/plugin-log";

function setBuffer(buffer: number[]) {
  invoke("set_buffer", { buffer });
}

// prettier-ignore
const buttons = [
  { 
    children: "⚫️ Blackout", 
    onClick: () => setBuffer([0, 0, 0, 0, 0, 0]) 
  },
  {
    children: "💡 Full Bright",
    onClick: () => setBuffer([255, 255, 255, 255, 255, 255]),
  },
  { children: "🌈 RGB Chase", 
    onClick: () => invoke("rgb_chase") 
  },
  {
    children: "✅ Default",
    onClick: () => setBuffer([121, 255, 255, 0, 0, 42]),
  },
  { 
    children: "🔄 Sync",
    onClick: () => invoke("sync_state") 
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
    <Button
      key={children}
      onClick={handleClick}
      className=""
      variant="ghost"
      size="sm"
    >
      {children}
    </Button>
  );
}

function ButtonRow() {
  return <div className="flex gap-3">{buttons.map(ControlButton)}</div>;
}

export default ButtonRow;
