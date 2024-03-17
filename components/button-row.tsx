"use client";

import { Button } from "@/components/ui/button";
import { invoke } from "@tauri-apps/api/tauri";

function BlackoutButton() {
  return (
    <Button
      onClick={() => invoke("blackout")}
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
      onClick={() => invoke("full_bright")}
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
      onClick={() => invoke("rgb_chase")}
      className=""
      variant="ghost"
      size="sm"
      disabled
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
