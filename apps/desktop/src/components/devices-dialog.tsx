import { useEffect, useState } from "react";
import { toast } from "sonner";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, MonitorSmartphone, Trash2, Wifi } from "lucide-react";
import { createTauRPCProxy, type PairedDevice, type PendingDevice } from "@/bindings";
import useAuth from "@/hooks/useAuth";
import useRemotePeers from "@/hooks/useRemotePeers";
import useSetups from "@/hooks/useSetups";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

const cmd = () => createTauRPCProxy().cmd;

/** Query key for the account's paired lux-node boxes (also used by the delete-account confirm). */
export const PAIRED_DEVICES_QUERY_KEY = ["pairedDevices"] as const;
/** Query key for the same-egress pending devices on the Add-device screen. */
export const PENDING_DEVICES_QUERY_KEY = ["pendingDevices"] as const;

/**
 * Settings → Devices: pair a headless lux-node box and manage the ones already
 * paired.
 *
 * Add flow (docs/claim-code-pairing.md): the box registers over HTTPS and the
 * backend only lists registrations that share the caller's public egress, so a
 * phone on the venue Wi-Fi sees the box on the venue ethernet — the human
 * confirms identity by matching the short code / MAC tail on the box's label,
 * picks the setup it should drive, and approves. The box's retained presence
 * card is the success signal, so a paired box flips to online (reusing the same
 * presence handling as the remote indicator) once it connects.
 *
 * Remove drops the box from the account registry; it disappears from this list.
 */
export default function DevicesDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const status = useAuth();
  const signedIn = !!status?.signedIn;
  const queryClient = useQueryClient();
  const setups = useSetups();
  const peers = useRemotePeers();

  // Transient UI state, reset whenever the dialog closes.
  const [expanded, setExpanded] = useState<string | null>(null); // pairRef of the device being approved
  const [chosenSetup, setChosenSetup] = useState<string | null>(null);
  const [busyRef, setBusyRef] = useState<string | null>(null); // pairRef mid-approve
  const [armedRemove, setArmedRemove] = useState<string | null>(null); // deviceId armed for removal
  useEffect(() => {
    if (!open) {
      setExpanded(null);
      setChosenSetup(null);
      setBusyRef(null);
      setArmedRemove(null);
    }
  }, [open]);

  const { data: paired } = useQuery({
    queryKey: PAIRED_DEVICES_QUERY_KEY,
    queryFn: () => cmd().list_paired_devices(),
    enabled: signedIn && open,
    // A just-approved box appears here and comes online within a poll or two.
    refetchInterval: open ? 3000 : false,
  });
  const { data: pending } = useQuery({
    queryKey: PENDING_DEVICES_QUERY_KEY,
    queryFn: () => cmd().list_pending_devices(),
    enabled: signedIn && open,
    refetchInterval: open ? 3000 : false,
  });

  const refresh = () => {
    queryClient.invalidateQueries({ queryKey: PAIRED_DEVICES_QUERY_KEY });
    queryClient.invalidateQueries({ queryKey: PENDING_DEVICES_QUERY_KEY });
  };

  // A paired box is "online" when a retained presence card matches its setup
  // and hostname — the node publishes `lux-node (<hostname>)` on the setup it
  // drives, the same signal the remote indicator reads.
  const isOnline = (d: PairedDevice) =>
    !!peers?.some((p) => p.setupId === d.setupId && p.name.includes(d.hostname));

  const toggleExpand = (pairRef: string) => {
    setExpanded((cur) => (cur === pairRef ? null : pairRef));
    setChosenSetup(null);
  };

  const approve = async (device: PendingDevice) => {
    if (!chosenSetup) {
      toast.error("Pick a setup for this device first.");
      return;
    }
    const setup = setups?.find((s) => s.id === chosenSetup);
    setBusyRef(device.pairRef);
    try {
      await cmd().approve_device(
        device.pairRef,
        chosenSetup,
        setup?.universe ?? null,
        null,
      );
      toast.success(`Approved ${device.hostname}. Waiting for it to come online…`);
      setExpanded(null);
      setChosenSetup(null);
      refresh();
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusyRef(null);
    }
  };

  const remove = async (device: PairedDevice) => {
    try {
      await cmd().remove_device(device.deviceId);
      toast.success(`Removed ${device.name}.`);
      setArmedRemove(null);
      refresh();
    } catch (e) {
      toast.error(String(e));
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Devices</DialogTitle>
          <DialogDescription>
            Pair a lux-node box and manage the ones already on your account.
          </DialogDescription>
        </DialogHeader>

        {!signedIn ? (
          <p className="py-4 text-sm text-muted-foreground">
            Sign in to pair and manage devices.
          </p>
        ) : (
          <div className="flex flex-col gap-5">
            {/* Paired devices */}
            <section className="flex flex-col gap-2">
              <h3 className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
                Paired
              </h3>
              {paired && paired.length > 0 ? (
                <ul className="flex flex-col gap-1.5">
                  {paired.map((d) => (
                    <li
                      key={d.deviceId}
                      className="flex items-center gap-3 rounded-md border px-3 py-2"
                    >
                      <span
                        className={cn(
                          "size-2 shrink-0 rounded-full",
                          isOnline(d) ? "bg-emerald-500" : "bg-muted-foreground/40",
                        )}
                        aria-label={isOnline(d) ? "online" : "offline"}
                      />
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-sm font-medium">{d.name}</div>
                        <div className="truncate text-xs text-muted-foreground">
                          {d.hostname} · universe {d.universe}
                        </div>
                      </div>
                      {armedRemove === d.deviceId ? (
                        <Button
                          variant="destructive"
                          size="sm"
                          onClick={() => remove(d)}
                        >
                          Confirm remove
                        </Button>
                      ) : (
                        <Button
                          variant="ghost"
                          size="sm"
                          className="text-muted-foreground hover:text-destructive"
                          aria-label={`Remove ${d.name}`}
                          onClick={() => setArmedRemove(d.deviceId)}
                        >
                          <Trash2 className="size-4" />
                        </Button>
                      )}
                    </li>
                  ))}
                </ul>
              ) : (
                <p className="text-sm text-muted-foreground">No paired devices yet.</p>
              )}
            </section>

            {/* Add device */}
            <section className="flex flex-col gap-2">
              <h3 className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
                Add a device
              </h3>
              {pending && pending.length > 0 ? (
                <ul className="flex flex-col gap-1.5">
                  {pending.map((p) => {
                    const isExpanded = expanded === p.pairRef;
                    return (
                      <li key={p.pairRef} className="rounded-md border">
                        <button
                          type="button"
                          className="flex w-full items-center gap-3 px-3 py-2 text-left"
                          onClick={() => toggleExpand(p.pairRef)}
                        >
                          <MonitorSmartphone className="size-4 shrink-0 text-muted-foreground" />
                          <div className="min-w-0 flex-1">
                            <div className="truncate text-sm font-medium">
                              {p.hostname}
                            </div>
                            <div className="truncate text-xs text-muted-foreground">
                              MAC …{p.macTail} · v{p.version}
                            </div>
                          </div>
                          <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-medium">
                            {p.userCode}
                          </code>
                        </button>
                        {isExpanded ? (
                          <div className="flex flex-col gap-2 border-t px-3 py-2">
                            <p className="text-xs text-muted-foreground">
                              Confirm <span className="font-medium">{p.userCode}</span>{" "}
                              and MAC …{p.macTail} on the box's label, then pick the
                              setup it should drive.
                            </p>
                            {setups && setups.length > 0 ? (
                              <div className="flex flex-wrap gap-1.5">
                                {setups.map((s) => (
                                  <Button
                                    key={s.id}
                                    variant={chosenSetup === s.id ? "default" : "outline"}
                                    size="sm"
                                    onClick={() => setChosenSetup(s.id)}
                                  >
                                    {chosenSetup === s.id ? (
                                      <Check className="size-3.5" />
                                    ) : null}
                                    {s.name}
                                    <span className="opacity-60">U{s.universe}</span>
                                  </Button>
                                ))}
                              </div>
                            ) : (
                              <p className="text-xs text-muted-foreground">
                                Create a setup first, then assign this device to it.
                              </p>
                            )}
                            <div className="flex justify-end">
                              <Button
                                size="sm"
                                disabled={!chosenSetup || busyRef === p.pairRef}
                                onClick={() => approve(p)}
                              >
                                {busyRef === p.pairRef ? "…" : "Approve"}
                              </Button>
                            </div>
                          </div>
                        ) : null}
                      </li>
                    );
                  })}
                </ul>
              ) : (
                <div className="flex items-start gap-2 rounded-md border border-dashed px-3 py-3 text-sm text-muted-foreground">
                  <Wifi className="mt-0.5 size-4 shrink-0" />
                  <span>
                    No devices found. Join the venue's Wi-Fi to add a box on the same
                    network — a device on cellular won't appear here.
                  </span>
                </div>
              )}
            </section>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
