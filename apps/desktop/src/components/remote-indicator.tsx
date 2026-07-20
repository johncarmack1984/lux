import { useRef } from "react";
import { Radio } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { createTauRPCProxy } from "@/bindings";
import useAuth from "@/hooks/useAuth";
import useRemotePeers from "@/hooks/useRemotePeers";
import useSetups from "@/hooks/useSetups";

/**
 * Live-rig indicator for the nav, shown only when signed in and another of the
 * user's devices has this setup active — meaning touches here apply there.
 * Brief absences are held for 5s before the indicator disappears, so the user
 * channel's hourly re-auth reconnect never visibly flickers it.
 */
const cmd = () => createTauRPCProxy().cmd;

export default function RemoteIndicator() {
  const auth = useAuth();
  const peers = useRemotePeers();
  const setups = useSetups();
  const lastLive = useRef<{ name: string; at: number } | null>(null);
  // A guest's presence card is keyed by their sub and carries their *device*
  // name, so on its own the indicator would say "iPhone" for a person. The
  // grant list is what turns that into who they are.
  const { data: shares } = useQuery({
    queryKey: ["grantedShares"],
    queryFn: () => cmd().list_granted_shares(),
    enabled: !!auth?.signedIn,
    refetchInterval: 30000,
  });

  if (!auth?.signedIn) return null;

  const active = setups?.find((s) => s.active);
  const live = active ? peers?.find((p) => p.setupId === active.id) : undefined;
  // A peer whose session matches a contact we granted this setup to is that
  // contact, on whatever device they happen to be holding.
  const guest = live
    ? shares?.granted.find(
        (g) => g.contactSub === live.session && g.setupId === active?.id,
      )
    : undefined;
  const liveName = guest ? (guest.label ?? guest.contactLabel) : live?.name;
  if (live && liveName) lastLive.current = { name: liveName, at: Date.now() };
  const held =
    !live && lastLive.current && Date.now() - lastLive.current.at < 5000
      ? lastLive.current
      : null;
  const shown = liveName ?? held?.name;
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
