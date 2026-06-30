import { useEffect, useState } from "react";
import { trace } from "@tauri-apps/plugin-log";
import { createTauRPCProxy, type DmxDeviceInfo } from "@/bindings";

/**
 * The detected DMX outputs and which one is active. Fetches on mount and stays
 * live via the `dmxDevicesChanged` event the backend emits on auto-detect,
 * rescan, or a manual pick. `null` while the first fetch is in flight.
 *
 * This backs the in-app output picker — the only output selector on mobile,
 * where there's no system tray.
 */
export default function useDmxDevices() {
  const [devices, setDevices] = useState<DmxDeviceInfo[] | null>(null);

  useEffect(() => {
    const taurpc = createTauRPCProxy();
    let active = true;

    taurpc.cmd
      .list_dmx_devices()
      .then((d) => {
        if (active) setDevices(d);
      })
      .catch((e) => trace(`list_dmx_devices failed: ${e}`));

    const unlisten = taurpc.cmd.event.on((event) => {
      if (event.type !== "dmxDevicesChanged") return;
      setDevices(event.devices);
    });

    return () => {
      active = false;
      unlisten.then((f) => f());
    };
  }, []);

  return devices;
}
