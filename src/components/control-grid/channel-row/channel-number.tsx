import type { ChannelProps, LuxLabelColor } from "@/global";
import { type CellContext } from "@tanstack/react-table";
import { cn, lightColorVariants } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { TableCell } from "@/components/ui/table";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

const labelColorOptions: LuxLabelColor[] = [
  "Red",
  "Green",
  "Blue",
  "Amber",
  "White",
  "Brightness",
];

const ColorOption = (labelColor: LuxLabelColor) => {
  const firstLetter = labelColor[0].toUpperCase();
  return (
    <DropdownMenuItem
      className="flex justify-end gap-4 w-full text-right"
      key={`${labelColor}-dropdown-item`}
    >
      {labelColor}
      <Button className={cn(lightColorVariants({ labelColor }))} size="icon">
        {firstLetter}
      </Button>
    </DropdownMenuItem>
  );
};

const ChannelNumber = ({ row }: CellContext<ChannelProps, unknown>) => {
  const { channelNumber, labelColor } = row.original;
  const key = `channel-number-${row.original.id}`;
  return (
    <TableCell className="w-5" id={key} key={key}>
      <DropdownMenu>
        <DropdownMenuTrigger>
          <div className={cn(lightColorVariants({ labelColor }))}>
            {channelNumber}
          </div>
        </DropdownMenuTrigger>
        <DropdownMenuContent className=" w-40" align="end">
          {labelColorOptions.map(ColorOption)}
        </DropdownMenuContent>
      </DropdownMenu>
    </TableCell>
  );
};

export default ChannelNumber;
