import type { LuxChannel } from "@/global";
import { info } from "@tauri-apps/plugin-log";
import { useCallback } from "react";

const toBoardDisplay = (fill: string, base: string) => base.padStart(3, fill);

const usePrintChannels = () => {
  const printChannels = useCallback(
    (channels: LuxChannel[] | null, buffer: number[]) => {
      if (channels === null) return;
      let l1 = "";
      let l2 = "";
      let length = channels.length;
      buffer.forEach((value, i) => {
        l1 += `| ${toBoardDisplay("0", String(value))}`;
        l2 += "| " + toBoardDisplay(" ", `C${i + 1}`);
      });
      l1 += "|";
      l2 += "|";
      info(l1);
      info(l2);
    },
    []
  );
  return printChannels;
};

export default usePrintChannels;
