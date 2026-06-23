import type { ChannelProps } from "@/global";
import { type CellContext } from "@tanstack/react-table";
import { useEffect, useState } from "react";
import { Slider } from "@/components/ui/slider";
import { TableCell } from "../../ui/table";
import { setChannelValue } from "@/lib/actions";
import { toast } from "sonner";
import useThrottle from "@/hooks/useThrottle";

const ChannelSlider = ({ row }: CellContext<ChannelProps, unknown>) => {
  const { id, channelNumber, value } = row.original;
  const [values, setValues] = useState([value]);
  // Slider drags fire continuously; keep the UI immediate but throttle the
  // hardware/IPC write so we don't flood the DMX render path (the old "judder").
  const sendValue = useThrottle((channel: number, next: number) => {
    setChannelValue({ channelNumber: channel, value: next }).catch(toast.error);
  }, 40);
  const dragSlider = (newValues: number[]) => {
    setValues(newValues);
    sendValue(channelNumber, newValues[0]);
  };
  useEffect(() => {
    setValues([value]);
  }, [value]);

  const key = `value-slider-${id}`;
  return (
    <TableCell className="w-full" id={key} key={key}>
      <Slider
        id={`${key}-slider`}
        value={values}
        onValueChange={dragSlider}
        max={255}
        step={1}
      />
    </TableCell>
  );
};

export default ChannelSlider;
