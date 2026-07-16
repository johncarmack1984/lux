import { useEffect, useState } from "react";
import { toast } from "sonner";
import type { ChannelProps } from "@/global";
import { setChannelValue } from "@/lib/actions";
import useThrottle from "@/hooks/useThrottle";

/**
 * The state machine behind one channel fader, shared by the horizontal
 * (DeskRow) and vertical (DeskColumn) layouts: local slider state for smooth
 * drags, a throttled IPC write, re-sync from out-of-band changes, and a 0/255
 * toggle.
 */
export default function useChannelFader(channel: ChannelProps) {
  const { channelNumber, value } = channel;
  const [values, setValues] = useState([value]);

  // Drags/clicks fire continuously; keep the UI immediate but throttle the
  // hardware/IPC write so we don't flood the DMX render path.
  const sendValue = useThrottle((next: number) => {
    setChannelValue({ channelNumber, value: next }).catch((e) =>
      toast.error(String(e))
    );
  }, 40);

  // Re-sync when the value changes from elsewhere (color picker, remote, or the
  // optimistic echo of our own write); harmless mid-drag since that echo equals
  // what we just sent.
  useEffect(() => {
    setValues([value]);
  }, [value]);

  const drag = (next: number[]) => {
    setValues(next);
    sendValue(next[0]);
  };

  const toggle = () => {
    const next = values[0] === 0 ? 255 : 0;
    setValues([next]);
    sendValue(next);
  };

  return { values, drag, toggle };
}
