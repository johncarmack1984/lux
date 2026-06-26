import { Cloud, CloudOff, RefreshCw } from "lucide-react";
import useAuth from "@/hooks/useAuth";
import useSyncStatus from "@/hooks/useSyncStatus";

/**
 * A small cloud-sync indicator for the nav, shown only when signed in. Reflects
 * the backend `SyncState`: a spinner while syncing, a cloud when synced, and a
 * struck-through cloud (with a hint) when offline. Idle renders nothing, so the
 * nav stays quiet when there's nothing to report.
 */
export default function SyncIndicator() {
  const auth = useAuth();
  const state = useSyncStatus();

  if (!auth?.signedIn) return null;

  switch (state) {
    case "syncing":
      return (
        <span title="Syncing…" className="text-muted-foreground">
          <RefreshCw className="size-4 animate-spin" aria-label="Syncing" />
        </span>
      );
    case "offline":
      return (
        <span
          title="Offline — your changes will sync when the connection returns"
          className="text-amber-500"
        >
          <CloudOff className="size-4" aria-label="Offline" />
        </span>
      );
    case "synced":
      return (
        <span title="Synced" className="text-muted-foreground">
          <Cloud className="size-4" aria-label="Synced" />
        </span>
      );
    default:
      return null;
  }
}
