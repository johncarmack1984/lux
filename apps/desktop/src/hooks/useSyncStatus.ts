import { useEffect, useState } from "react";
import { trace } from "@tauri-apps/plugin-log";
import { createTauRPCProxy, type SyncState } from "@/bindings";

/**
 * The current cloud-sync state (idle / syncing / synced / offline). Fetches on
 * mount and stays live via the `syncStatusChanged` event the backend emits as a
 * push/pull cycle progresses. `null` until the first fetch resolves.
 */
export default function useSyncStatus() {
  const [state, setState] = useState<SyncState | null>(null);

  useEffect(() => {
    const taurpc = createTauRPCProxy();
    let active = true;

    taurpc.cmd
      .sync_status()
      .then((s) => {
        if (active) setState(s);
      })
      .catch((e) => trace(`sync_status failed: ${e}`));

    const unlisten = taurpc.cmd.event.on((event) => {
      if (event.type !== "syncStatusChanged") return;
      setState(event.state);
    });

    return () => {
      active = false;
      unlisten.then((f) => f());
    };
  }, []);

  return state;
}
