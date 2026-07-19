import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import {
  Check,
  ChevronsUpDown,
  Pencil,
  Plus,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { createTauRPCProxy, type SetupSummary } from "@/bindings";
import useSetups from "@/hooks/useSetups";
import useDmxDevices, { DMX_DEVICES_QUERY_KEY } from "@/hooks/useDmxDevices";
import useLuxRefresh from "@/hooks/useLuxRefresh";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { cn } from "@/lib/utils";

const cmd = () => createTauRPCProxy().cmd;

/** A setup row's in-flight edits (or the create form's, with id null). */
type Draft = { id: string | null; name: string; universe: number };

/**
 * One dropdown for everything setups: each row switches on click and edits in
 * place (the pencil flips the row to name/universe inputs — any setup, not
 * just the active one), a "New setup…" row expands the same way, and the DMX
 * output picker keeps its spot at the bottom. A Popover rather than a
 * DropdownMenu because menu items fight inline inputs for focus and keys.
 */
export default function SetupSwitcher() {
  const setups = useSetups();
  const active = setups?.find((s) => s.active) ?? null;
  const dmxDevices = useDmxDevices();
  const refresh = useLuxRefresh();
  const queryClient = useQueryClient();
  const refreshDevices = () =>
    queryClient.invalidateQueries({ queryKey: DMX_DEVICES_QUERY_KEY });

  const [open, setOpen] = useState(false);
  // Seeded when a row's pencil (or "New setup…") is clicked, never from
  // background refreshes, so a `setupsChanged` mid-edit can't clobber typing.
  const [draft, setDraft] = useState<Draft | null>(null);
  const [rescanning, setRescanning] = useState(false);
  // Deleting a setup takes its fixtures with it and there is no undo, so the
  // trash arms on the first tap (turns destructive) and only deletes on the
  // second; it disarms itself after a moment or when the dropdown closes.
  const [armedDelete, setArmedDelete] = useState<string | null>(null);
  const disarmTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(
    () => () => {
      if (disarmTimer.current) clearTimeout(disarmTimer.current);
    },
    [],
  );

  if (!setups || !active) {
    return (
      <Button variant="outline" size="sm" disabled>
        Setup
      </Button>
    );
  }

  const onOpenChange = (next: boolean) => {
    setOpen(next);
    if (!next) {
      setDraft(null);
      setArmedDelete(null);
    }
  };

  const switchTo = (id: string) => {
    setOpen(false);
    if (id === active.id) return;
    cmd()
      .set_active_setup(id)
      .then(refresh)
      .catch((e) => toast.error(String(e)));
  };

  const saveDraft = async () => {
    if (!draft) return;
    const name = draft.name.trim();
    if (!name) return;
    try {
      if (draft.id === null) {
        const before = setups;
        const after: SetupSummary[] = await cmd().create_setup(
          name,
          draft.universe,
        );
        const created = after.find((s) => !before.some((b) => b.id === s.id));
        if (created) await cmd().set_active_setup(created.id);
        setOpen(false);
      } else {
        const current = setups.find((s) => s.id === draft.id);
        if (current && name !== current.name)
          await cmd().rename_setup(draft.id, name);
        if (current && draft.universe !== current.universe)
          await cmd().set_setup_universe(draft.id, draft.universe);
      }
      await refresh();
      setDraft(null);
    } catch (e) {
      toast.error(String(e));
    }
  };

  const remove = async (id: string) => {
    try {
      await cmd().delete_setup(id);
      await refresh();
    } catch (e) {
      toast.error(String(e));
    }
  };

  const onTrash = (id: string) => {
    if (armedDelete === id) {
      setArmedDelete(null);
      void remove(id);
      return;
    }
    setArmedDelete(id);
    if (disarmTimer.current) clearTimeout(disarmTimer.current);
    disarmTimer.current = setTimeout(() => setArmedDelete(null), 3000);
  };

  const selectDevice = (key: string) => {
    cmd()
      .set_dmx_device(key)
      .then(refreshDevices)
      .catch((e) => toast.error(String(e)));
  };

  // Detection runs ~3s on the backend and reports back via `dmxDevicesChanged`;
  // show the spinner across that window rather than block on the call's return,
  // and refetch the list when it closes (the event doesn't reach iOS).
  const rescanDevices = async () => {
    setRescanning(true);
    try {
      await cmd().rescan_dmx_devices();
    } catch (e) {
      toast.error(String(e));
    }
    setTimeout(() => {
      setRescanning(false);
      void refreshDevices();
    }, 3500);
  };

  const editorFor = (heading: string, confirmLabel: string) => (
    <div className="flex flex-col gap-2 rounded-md border p-2">
      <p className="text-xs font-medium text-muted-foreground">{heading}</p>
      <Input
        autoFocus
        value={draft!.name}
        onChange={(e) => setDraft({ ...draft!, name: e.target.value })}
        onKeyDown={(e) => {
          if (e.key === "Enter") void saveDraft();
        }}
        placeholder="Name"
        className="h-8"
      />
      <label className="flex items-center justify-between gap-2 text-xs font-medium text-muted-foreground">
        Universe
        <Input
          type="number"
          min={1}
          max={63999}
          className="h-8 w-24"
          value={draft!.universe}
          onChange={(e) => setDraft({ ...draft!, universe: Number(e.target.value) })}
        />
      </label>
      <div className="flex gap-2">
        <Button
          size="sm"
          className="flex-1"
          onClick={saveDraft}
          disabled={!draft!.name.trim()}
        >
          {confirmLabel}
        </Button>
        <Button size="sm" variant="ghost" onClick={() => setDraft(null)}>
          Cancel
        </Button>
      </div>
    </div>
  );

  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <PopoverTrigger asChild>
        <Button variant="outline" size="sm" className="gap-1.5">
          <span className="max-w-32 truncate">{active.name}</span>
          <ChevronsUpDown className="size-3.5 text-muted-foreground" />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-72 p-2">
        <div className="flex flex-col gap-2">
          <div className="flex flex-col gap-0.5">
            <p className="px-2 py-1 text-xs font-medium text-muted-foreground">
              Setups
            </p>
            {setups.map((s) =>
              draft?.id === s.id ? (
                <div key={s.id}>{editorFor(s.name, "Save")}</div>
              ) : (
                <div
                  key={s.id}
                  className="flex items-center rounded-md hover:bg-accent"
                >
                  <button
                    type="button"
                    onClick={() => switchTo(s.id)}
                    className="flex min-w-0 flex-1 items-center gap-2 px-2 py-1.5 text-left text-sm"
                  >
                    <Check
                      className={cn(
                        "size-4 shrink-0",
                        s.active ? "opacity-100" : "opacity-0",
                      )}
                    />
                    <span className="flex-1 truncate">{s.name}</span>
                    <span className="text-xs text-muted-foreground">
                      U{s.universe} · {s.fixtureCount}fx
                    </span>
                  </button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="size-7 shrink-0 text-muted-foreground hover:text-foreground"
                    aria-label={`Edit ${s.name}`}
                    onClick={() =>
                      setDraft({ id: s.id, name: s.name, universe: s.universe })
                    }
                  >
                    <Pencil className="size-3.5" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className={cn(
                      "size-7 shrink-0",
                      armedDelete === s.id
                        ? "bg-destructive/15 text-destructive hover:text-destructive"
                        : "text-muted-foreground hover:text-destructive",
                    )}
                    aria-label={
                      armedDelete === s.id
                        ? `Tap again to delete ${s.name}`
                        : `Delete ${s.name}`
                    }
                    disabled={setups.length <= 1}
                    onClick={() => onTrash(s.id)}
                  >
                    <Trash2 className="size-3.5" />
                  </Button>
                </div>
              ),
            )}
            {draft?.id === null ? (
              editorFor("New setup", "Create & switch")
            ) : (
              <button
                type="button"
                onClick={() => setDraft({ id: null, name: "", universe: 1 })}
                className="flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm text-muted-foreground hover:bg-accent hover:text-foreground"
              >
                <Plus className="size-4 shrink-0" />
                New setup…
              </button>
            )}
          </div>

          <div className="h-px bg-border" />

          <div className="flex flex-col gap-1 px-1 pb-1">
            <div className="flex items-center justify-between">
              <p className="px-1 text-xs font-medium text-muted-foreground">
                DMX output
              </p>
              <Button
                variant="ghost"
                size="sm"
                className="h-6 gap-1 px-2 text-xs text-muted-foreground"
                onClick={rescanDevices}
                disabled={rescanning}
              >
                <RefreshCw
                  className={cn("size-3", rescanning && "animate-spin")}
                />
                Rescan
              </Button>
            </div>
            {dmxDevices === null ? (
              <p className="px-1 text-xs text-muted-foreground">Scanning…</p>
            ) : dmxDevices.length === 0 ? (
              <p className="px-1 text-xs text-muted-foreground">
                No outputs found.
              </p>
            ) : (
              <div className="flex flex-col gap-0.5">
                {dmxDevices.map((d) => (
                  <button
                    key={d.key}
                    type="button"
                    onClick={() => selectDevice(d.key)}
                    className={cn(
                      "flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm hover:bg-accent",
                      d.active && "bg-accent/50",
                    )}
                  >
                    <Check
                      className={cn(
                        "size-4 shrink-0",
                        d.active ? "opacity-100" : "opacity-0",
                      )}
                    />
                    <span className="flex-1 truncate">{d.label}</span>
                  </button>
                ))}
              </div>
            )}
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
}
