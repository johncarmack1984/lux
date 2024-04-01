"use client";

import useBuffer from "@/hooks/useBuffer";
import { useCallback } from "react";

function BufferState() {
  const buffer = useBuffer();

  const bufferDisplay = useCallback(
    (n: number, i: number) =>
      n.toString().padStart(3, "0") + `${i + 1 === buffer?.length ? "" : ","}`,
    [buffer]
  );
  if (!buffer) return null;

  return (
    <div className="flex flex-col gap-2 my-12 justify-between font-mono items-center">
      {!!buffer ? <>[{buffer?.map(bufferDisplay)}]</> : <>null</>}
    </div>
  );
}

export default BufferState;
