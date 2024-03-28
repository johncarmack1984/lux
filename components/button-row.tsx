"use client";

import { Button } from "@/components/ui/button";
import { invoke } from "@tauri-apps/api/core";
// import { emit } from "@tauri-apps/api/event";
import { debug } from "@tauri-apps/plugin-log";

function BlackoutButton() {
  return (
    <Button
      // onClick={() => invoke("blackout")}
      onClick={() => {
        debug("blackout");
        invoke("full_bright");
      }}
      className=""
      variant="ghost"
      size="sm"
    >
      ⚫️ Blackout
    </Button>
  );
}

function FullBrightButton() {
  return (
    <Button
      onClick={() => {
        debug("full_bright");
        invoke("full_bright");
      }}
      className=""
      variant="ghost"
      size="sm"
    >
      💡 Full Bright
    </Button>
  );
}

function RgbChaseButton() {
  return (
    <Button
      onClick={() => {
        debug("rgb_chase");
        // emit("rgb_chase")
      }}
      className=""
      variant="ghost"
      size="sm"
    >
      🌈 RGB Chase
    </Button>
  );
}

function ButtonRow() {
  return (
    <div className="flex gap-3">
      <BlackoutButton />
      <FullBrightButton />
      <RgbChaseButton />
    </div>
  );
}

export default ButtonRow;
