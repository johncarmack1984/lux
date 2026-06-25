import { useEffect, useState } from "react";
import { trace } from "@tauri-apps/plugin-log";
import { createTauRPCProxy, type SetupSummary } from "@/bindings";

/**
 * The user's setups and which one is active. Fetches on mount and stays live via
 * the `setupsChanged` event the backend emits on create/rename/delete/switch.
 * `null` while the first fetch is in flight.
 */
export default function useSetups() {
  const [setups, setSetups] = useState<SetupSummary[] | null>(null);

  useEffect(() => {
    const taurpc = createTauRPCProxy();
    let active = true;

    taurpc.cmd
      .list_setups()
      .then((s) => {
        if (active) setSetups(s);
      })
      .catch((e) => trace(`list_setups failed: ${e}`));

    const unlisten = taurpc.cmd.event.on((event) => {
      if (event.type !== "setupsChanged") return;
      setSetups(event.setups);
    });

    return () => {
      active = false;
      unlisten.then((f) => f());
    };
  }, []);

  return setups;
}
