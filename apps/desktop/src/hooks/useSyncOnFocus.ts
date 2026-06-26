import { useEffect } from "react";
import { trace } from "@tauri-apps/plugin-log";
import { createTauRPCProxy } from "@/bindings";

/** Minimum gap between focus-triggered pulls, so rapid focus toggles don't spam. */
const MIN_INTERVAL_MS = 15_000;

/**
 * Pull the account's setups whenever the window regains focus or becomes
 * visible, so edits made on another device land without waiting for a restart.
 * Throttled to {@link MIN_INTERVAL_MS}; the backend also coalesces concurrent
 * syncs and no-ops when signed out, so this is safe to leave always-on.
 */
export default function useSyncOnFocus() {
  useEffect(() => {
    const taurpc = createTauRPCProxy();
    let last = 0;

    const pull = () => {
      const now = Date.now();
      if (now - last < MIN_INTERVAL_MS) return;
      last = now;
      taurpc.cmd.sync_now().catch((e) => trace(`sync_now failed: ${e}`));
    };
    const onVisible = () => {
      if (document.visibilityState === "visible") pull();
    };

    window.addEventListener("focus", pull);
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      window.removeEventListener("focus", pull);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, []);
}
