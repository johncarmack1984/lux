"use client";

import { Button } from "@/components/ui/button";
// import type { LuxClient } from "@/global";
// import useTauRPC from "@/hooks/useTauRPC";
import { invoke } from "@tauri-apps/api/core";
// import { emit } from "@tauri-apps/api/event";
import { debug, error, trace } from "@tauri-apps/plugin-log";

const setBuffer = (buffer: number[]) => {
  invoke("buffer/set", { buffer });
};

// async function getInitialState() {
//   return await invoke("get_initial_state")
//     .then((state) => {
//       trace(`frontend received ${JSON.stringify(state)}`);
//       return state;
//     })
//     .catch(error);
// }

const buttons = [
  {
    children: "⚫️ Blackout",
    // onClick: async (taurpc: LuxClient) => setBuffer([0, 0, 0, 0, 0, 0]),
    onClick: async () => setBuffer([0, 0, 0, 0, 0, 0]),
  },
  {
    children: "✅ Default",
    onClick: () => setBuffer([121, 255, 255, 0, 0, 42]),
  },
  {
    children: "💡 Full Bright",
    onClick: () =>
      // await taurpc.buffer.set([255, 255, 255, 255, 255, 255]),
      setBuffer([255, 255, 255, 255, 255, 255]),
  },
  // { children: "🌈 RGB Chase",
  //   onClick: () => invoke("rgb_chase")
  // },
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
  onClick: () => any;
}) {
  // const taurpc = useTauRPC();
  const handleClick = () => {
    trace(`frontend sending ${children}`);
    // if (!taurpc) {
    //   error("TauRPC not ready");
    //   return;
    // }
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
    <div className="grid-cols-3 py-8 grid">{buttons.map(ControlButton)}</div>
  );
}

export default ButtonRow;
