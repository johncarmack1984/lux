import {
  Table,
  TableBody,
  TableCell,
  TableFooter,
  TableRow,
} from "@/components/ui/table";
import { createTauRPCProxy } from "@/bindings";
import { Button } from "@/components/ui/button";
import { PlusIcon } from "lucide-react";
import useBuffer from "@/hooks/useBuffer";

function GridFooter() {
  const buffer = useBuffer();
  const nextChannelNumber = (buffer?.length ?? 0) + 1;
  const handleClick = async () => {
    const taurpc = createTauRPCProxy();
    await taurpc.cmd.insert_channel({
      id: crypto.randomUUID(),
      disabled: false,
      channelNumber: nextChannelNumber,
      label: "Channel",
      labelColor: "White",
    });
  };
  return (
    <TableFooter>
      <TableRow>
        <TableCell colSpan={5} className="text-center">
          <Button
            size="sm"
            onClick={handleClick}
            variant="outline"
            className="mx-auto m-5 p-5"
          >
            <PlusIcon size={12} className="size-4 mr-2" />
            Channel {nextChannelNumber}
          </Button>
        </TableCell>
      </TableRow>
    </TableFooter>
  );
}

export default GridFooter;
