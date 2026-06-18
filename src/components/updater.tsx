"use client";

import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import { useEffect } from "react";
import { toast } from "sonner";

/**
 * Checks for an update on startup and, if one is available, offers a one-click
 * "Update & Restart" toast. The signed `latest.json` for the release is fetched
 * from the GitHub "latest" release (see tauri.conf.json → plugins.updater).
 * No-ops quietly in dev or when the updater isn't reachable.
 */
export function Updater() {
  useEffect(() => {
    let cancelled = false;

    (async () => {
      try {
        const update = await check();
        if (cancelled || !update) return;

        toast(`Update ${update.version} available`, {
          description: update.body || "A new version of lux is ready to install.",
          duration: Infinity,
          action: {
            label: "Update & Restart",
            onClick: async () => {
              const id = toast.loading("Downloading update…");
              try {
                await update.downloadAndInstall();
                toast.dismiss(id);
                await relaunch();
              } catch (e) {
                toast.error(`Update failed: ${e}`, { id });
              }
            },
          },
        });
      } catch (e) {
        // Unavailable in dev / unsigned local runs, or the endpoint is unreachable.
        console.debug("update check skipped:", e);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, []);

  return null;
}
