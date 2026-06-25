import { useState } from "react";
import { toast } from "sonner";
import { Check, ChevronsUpDown, Plus, Settings2, Trash2 } from "lucide-react";
import { createTauRPCProxy, type SetupSummary } from "@/bindings";
import useSetups from "@/hooks/useSetups";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { cn } from "@/lib/utils";

const cmd = () => createTauRPCProxy().cmd;

/**
 * Switch between the user's setups and manage them (rename / rebind universe /
 * create / delete). Switching is the common action, so it's a plain dropdown;
 * the editing lives behind a "manage" popover to keep the nav uncluttered.
 */
export default function SetupSwitcher() {
  const setups = useSetups();
  const active = setups?.find((s) => s.active) ?? null;

  const [manageOpen, setManageOpen] = useState(false);
  const [editName, setEditName] = useState("");
  const [editUniverse, setEditUniverse] = useState(1);
  const [newName, setNewName] = useState("");
  const [newUniverse, setNewUniverse] = useState(1);

  if (!setups || !active) {
    return (
      <Button variant="outline" size="sm" disabled>
        Setup
      </Button>
    );
  }

  const switchTo = (id: string) => {
    if (id === active.id) return;
    cmd()
      .set_active_setup(id)
      .catch((e) => toast.error(String(e)));
  };

  // Seed the edit fields from the active setup only as the popover opens, so a
  // background `setupsChanged` can't clobber what the user is typing.
  const onManageOpenChange = (open: boolean) => {
    setManageOpen(open);
    if (open) {
      setEditName(active.name);
      setEditUniverse(active.universe);
      setNewName("");
      setNewUniverse(1);
    }
  };

  const saveCurrent = async () => {
    const name = editName.trim();
    if (!name) return;
    try {
      if (name !== active.name) await cmd().rename_setup(active.id, name);
      if (editUniverse !== active.universe)
        await cmd().set_setup_universe(active.id, editUniverse);
      setManageOpen(false);
    } catch (e) {
      toast.error(String(e));
    }
  };

  const removeCurrent = async () => {
    try {
      await cmd().delete_setup(active.id);
      setManageOpen(false);
    } catch (e) {
      toast.error(String(e));
    }
  };

  const createAndSwitch = async () => {
    const name = newName.trim();
    if (!name) return;
    try {
      const before = setups;
      const after: SetupSummary[] = await cmd().create_setup(name, newUniverse);
      const created = after.find((s) => !before.some((b) => b.id === s.id));
      if (created) await cmd().set_active_setup(created.id);
      setManageOpen(false);
    } catch (e) {
      toast.error(String(e));
    }
  };

  return (
    <div className="flex items-center gap-1">
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="outline" size="sm" className="gap-1.5">
            <span className="max-w-32 truncate">{active.name}</span>
            <ChevronsUpDown className="size-3.5 text-muted-foreground" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-56">
          <DropdownMenuLabel>Setups</DropdownMenuLabel>
          {setups.map((s) => (
            <DropdownMenuItem
              key={s.id}
              onSelect={() => switchTo(s.id)}
              className="gap-2"
            >
              <Check
                className={cn("size-4", s.active ? "opacity-100" : "opacity-0")}
              />
              <span className="flex-1 truncate">{s.name}</span>
              <span className="text-xs text-muted-foreground">
                U{s.universe} · {s.fixtureCount}fx
              </span>
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>

      <Popover open={manageOpen} onOpenChange={onManageOpenChange}>
        <PopoverTrigger asChild>
          <Button
            variant="ghost"
            size="icon"
            className="size-8"
            aria-label="Manage setups"
          >
            <Settings2 className="size-4" />
          </Button>
        </PopoverTrigger>
        <PopoverContent align="start" className="w-72">
          <div className="flex flex-col gap-4">
            <div className="flex flex-col gap-2">
              <p className="text-xs font-medium text-muted-foreground">
                Current setup
              </p>
              <Input
                value={editName}
                onChange={(e) => setEditName(e.target.value)}
                placeholder="Name"
              />
              <label className="flex items-center justify-between gap-2 text-xs font-medium text-muted-foreground">
                Universe
                <Input
                  type="number"
                  min={1}
                  max={63999}
                  className="h-8 w-24"
                  value={editUniverse}
                  onChange={(e) => setEditUniverse(Number(e.target.value))}
                />
              </label>
              <div className="flex gap-2">
                <Button
                  size="sm"
                  className="flex-1"
                  onClick={saveCurrent}
                  disabled={!editName.trim()}
                >
                  Save
                </Button>
                <Button
                  size="sm"
                  variant="destructive"
                  onClick={removeCurrent}
                  disabled={setups.length <= 1}
                  aria-label="Delete this setup"
                >
                  <Trash2 className="size-4" />
                </Button>
              </div>
            </div>

            <div className="h-px bg-border" />

            <div className="flex flex-col gap-2">
              <p className="text-xs font-medium text-muted-foreground">
                New setup
              </p>
              <Input
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                placeholder="e.g. Church"
              />
              <label className="flex items-center justify-between gap-2 text-xs font-medium text-muted-foreground">
                Universe
                <Input
                  type="number"
                  min={1}
                  max={63999}
                  className="h-8 w-24"
                  value={newUniverse}
                  onChange={(e) => setNewUniverse(Number(e.target.value))}
                />
              </label>
              <Button
                size="sm"
                onClick={createAndSwitch}
                disabled={!newName.trim()}
              >
                <Plus className="mr-1 size-4" /> Create &amp; switch
              </Button>
            </div>
          </div>
        </PopoverContent>
      </Popover>
    </div>
  );
}
