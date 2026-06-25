import { useEffect, useState } from "react";
import { trace } from "@tauri-apps/plugin-log";
import { createTauRPCProxy, type AuthStatus } from "@/bindings";

/**
 * The current account status: whether accounts are configured, whether someone
 * is signed in, and their email. Fetches on mount and stays live via the
 * `authChanged` event the backend emits on sign-in / sign-out / silent restore.
 * `null` while the first fetch is in flight.
 */
export default function useAuth() {
  const [status, setStatus] = useState<AuthStatus | null>(null);

  useEffect(() => {
    const taurpc = createTauRPCProxy();
    let active = true;

    taurpc.cmd
      .auth_status()
      .then((s) => {
        if (active) setStatus(s);
      })
      .catch((e) => trace(`auth_status failed: ${e}`));

    const unlisten = taurpc.cmd.event.on((event) => {
      if (event.type !== "authChanged") return;
      setStatus(event.status);
    });

    return () => {
      active = false;
      unlisten.then((f) => f());
    };
  }, []);

  return status;
}
