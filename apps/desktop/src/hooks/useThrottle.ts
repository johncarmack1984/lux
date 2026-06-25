import { useCallback, useEffect, useRef } from "react";

/**
 * Leading + trailing throttle for a callback. The UI can fire this as fast as it
 * likes (slider drags, color-wheel moves) while the wrapped call runs at most
 * once per `ms`, and the final value in a burst is always delivered (trailing
 * edge). Returns a stable identity across renders.
 */
export default function useThrottle<A extends unknown[]>(
  fn: (...args: A) => void,
  ms: number
): (...args: A) => void {
  const fnRef = useRef(fn);
  fnRef.current = fn;

  const last = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pending = useRef<A | null>(null);

  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    []
  );

  return useCallback(
    (...args: A) => {
      const now = Date.now();
      const remaining = ms - (now - last.current);
      pending.current = args;

      if (remaining <= 0) {
        if (timer.current) {
          clearTimeout(timer.current);
          timer.current = null;
        }
        last.current = now;
        fnRef.current(...args);
      } else if (!timer.current) {
        timer.current = setTimeout(() => {
          timer.current = null;
          last.current = Date.now();
          if (pending.current) fnRef.current(...pending.current);
        }, remaining);
      }
    },
    [ms]
  );
}
