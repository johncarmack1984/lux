"use client";

import {
  TableCaption,
  TableFooter,
  TableHead,
  TableHeader,
} from "@/components/ui/table";
import ButtonRow from "../button-row";
import { useCallback } from "react";

import useBuffer from "@/hooks/useBuffer";

function ControlGridCaption() {
  const buffer = useBuffer();

  const bufferDisplay = useCallback(
    (n: number, i: number) =>
      n.toString().padStart(3, "0") + `${i + 1 === buffer?.length ? "" : ","}`,
    [buffer]
  );

  return (
    <TableCaption className="text-base">
      <div className="flex flex-col gap-2 mb-6 justify-between font-mono items-center">
        {!!buffer ? <>[{buffer?.map(bufferDisplay)}]</> : <>null</>}
        <ButtonRow />
      </div>
    </TableCaption>
  );
}

export default ControlGridCaption;
