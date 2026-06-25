import { useEffect, useState } from "react";
import { trace } from "@tauri-apps/plugin-log";
import { createTauRPCProxy, type Fixture } from "@/bindings";

/**
 * The patched fixtures. Fetches the patch on mount and stays live via the
 * `patchSet` event the backend emits on every add/remove/update. `null` while
 * the first fetch is in flight.
 */
export default function useFixtures() {
  const [fixtures, setFixtures] = useState<Fixture[] | null>(null);

  useEffect(() => {
    const taurpc = createTauRPCProxy();
    let active = true;

    taurpc.cmd
      .get_patch()
      .then((f) => {
        if (active) setFixtures(f);
      })
      .catch((e) => trace(`get_patch failed: ${e}`));

    const unlisten = taurpc.cmd.event.on((event) => {
      if (event.type !== "patchSet") return;
      setFixtures(event.fixtures);
    });

    return () => {
      active = false;
      unlisten.then((f) => f());
    };
  }, []);

  return fixtures;
}
