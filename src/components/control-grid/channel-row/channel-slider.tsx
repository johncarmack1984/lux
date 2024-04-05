"use client";

import type { ChannelProps } from "@/global";
import { type CellContext } from "@tanstack/react-table";
import { useEffect, useState } from "react";
import { Slider } from "@/components/ui/slider";
import { TableCell } from "../../ui/table";
import { setChannelValue } from "@/app/actions";

const ChannelSlider = ({ row }: CellContext<ChannelProps, unknown>) => {
  const { id, channel_number: channelNumber, value } = row.original;
  const [values, setValues] = useState([value]);
  const dragSlider = async (newValues: number[]) => {
    setValues(newValues);
    await setChannelValue({ channelNumber, value: newValues[0] });
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
