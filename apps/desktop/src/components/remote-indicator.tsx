import { useRef } from "react";
import { Radio } from "lucide-react";
import useAuth from "@/hooks/useAuth";
import useRemotePeers from "@/hooks/useRemotePeers";
import useSetups from "@/hooks/useSetups";

/**
 * Live-rig indicator for the nav, shown only when signed in and another of the
 * user's devices has this setup active — meaning touches here apply there.
 * Brief absences are held for 5s before the indicator disappears, so the user
 * channel's hourly re-auth reconnect never visibly flickers it.
 */
export default function RemoteIndicator() {
  const auth = useAuth();
  const peers = useRemotePeers();
  const setups = useSetups();
  const lastLive = useRef<{ name: string; at: number } | null>(null);

  if (!auth?.signedIn) return null;

  const active = setups?.find((s) => s.active);
  const live = active ? peers?.find((p) => p.setupId === active.id) : undefined;
  if (live) lastLive.current = { name: live.name, at: Date.now() };
  const held =
    !live && lastLive.current && Date.now() - lastLive.current.at < 5000
      ? lastLive.current
      : null;
  const shown = live?.name ?? held?.name;
  if (!shown) return null;

  return (
    <span
      title={`Live — control also applies at ${shown}`}
      className="flex items-center gap-1.5 text-emerald-600 dark:text-emerald-500"
    >
      <Radio className="size-4" aria-label="Remote rig live" />
      <span className="hidden text-xs font-medium sm:inline">{shown}</span>
    </span>
  );
}
