import { useQuery } from "@tanstack/react-query";
import { createTauRPCProxy, type DmxDeviceInfo } from "@/bindings";

/** Query key for the detected DMX outputs. */
export const DMX_DEVICES_QUERY_KEY = ["dmxDevices"] as const;

/**
 * The detected DMX outputs and which one is active. `null` while the first read
 * is in flight. Backs the in-app output picker — the only output selector on
 * mobile, where there's no system tray.
 *
 * The backend emits `dmxDevicesChanged` on auto-detect / rescan / pick, but it
 * doesn't reach the webview on iOS. So poll a few times just after mount to
 * catch the ~3s startup auto-detect, then stop; the setup switcher invalidates
 * this key after a pick or a rescan.
 */
export default function useDmxDevices(): DmxDeviceInfo[] | null {
  const { data } = useQuery({
    queryKey: DMX_DEVICES_QUERY_KEY,
    queryFn: () => createTauRPCProxy().cmd.list_dmx_devices(),
    refetchInterval: (query) => (query.state.dataUpdateCount < 3 ? 2500 : false),
  });
  return data ?? null;
}
