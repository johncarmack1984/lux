import { useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  createTauRPCProxy,
  type SliderOrientation,
  type UserSettings,
} from "@/bindings";

/** Query key for the user's synced settings. */
export const SETTINGS_QUERY_KEY = ["settings"] as const;

/**
 * The user's settings (persisted locally, cloud-synced when signed in). `null`
 * until the first read resolves.
 *
 * The setter in `lib/actions` pushes the settings the backend returns straight
 * into the cache (the iOS-safe path); on desktop the `settingsChanged` event is
 * also honored as a fast path so a cloud pull from another device shows live.
 */
export default function useSettings(): UserSettings | null {
  const queryClient = useQueryClient();

  const { data } = useQuery({
    queryKey: SETTINGS_QUERY_KEY,
    queryFn: () => createTauRPCProxy().cmd.get_settings(),
  });

  useEffect(() => {
    const unlisten = createTauRPCProxy().cmd.event.on((event) => {
      if (event.type === "settingsChanged") {
        queryClient.setQueryData(SETTINGS_QUERY_KEY, event.settings);
      }
    });
    return () => {
      // .catch: if registration itself rejected (webview teardown), cleanup
      // must not surface an unhandled rejection.
      unlisten.then((f) => f()).catch(() => {});
    };
  }, [queryClient]);

  return data ?? null;
}

/**
 * The desk's fader orientation, defaulting to vertical (the classic
 * lighting-console layout) while settings load or when the field is absent.
 */
export function useSliderOrientation(): SliderOrientation {
  return useSettings()?.sliderOrientation ?? "vertical";
}
