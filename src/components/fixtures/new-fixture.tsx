import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Plus, X } from "lucide-react";
import {
  createTauRPCProxy,
  type ChannelDef,
  type Fixture,
  type FixturePreset,
  type LuxLabelColor,
} from "@/bindings";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";

const ROLES: LuxLabelColor[] = [
  "Red",
  "Green",
  "Blue",
  "Amber",
  "White",
  "Brightness",
  "Generic",
];

export default function NewFixture({ fixtures }: { fixtures: Fixture[] }) {
  const [presets, setPresets] = useState<FixturePreset[]>([]);
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [address, setAddress] = useState(1);
  const [channels, setChannels] = useState<ChannelDef[]>([]);

  useEffect(() => {
    createTauRPCProxy()
      .cmd.list_presets()
      .then(setPresets)
      .catch(() => {});
  }, []);

  // On open, suggest the first free slot after the last patched fixture.
  useEffect(() => {
    if (!open) return;
    const nextFree = fixtures.reduce(
      (max, f) => Math.max(max, f.address + f.channels.length),
      1
    );
    setAddress(Math.min(nextFree, 512));
  }, [open, fixtures]);

  const applyPreset = (preset: FixturePreset) => {
    setName((current) => current || preset.name);
    setChannels(preset.channels.map((c) => ({ ...c })));
  };

  const setChannel = (i: number, patch: Partial<ChannelDef>) =>
    setChannels((cs) => cs.map((c, j) => (j === i ? { ...c, ...patch } : c)));

  const end = address + channels.length - 1;
  const fits = channels.length > 0 && address >= 1 && end <= 512;

  const submit = async () => {
    try {
      await createTauRPCProxy().cmd.add_fixture(
        name.trim() || "Fixture",
        address,
        channels
      );
      setOpen(false);
      setName("");
      setChannels([]);
    } catch (e) {
      toast.error(String(e));
    }
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button size="sm">
          <Plus className="mr-1 size-4" /> New fixture
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-80">
        <div className="flex flex-col gap-3">
          <div>
            <p className="mb-1 text-xs font-medium text-muted-foreground">
              Start from a preset
            </p>
            <div className="flex flex-wrap gap-1">
              {presets.map((preset) => (
                <Button
                  key={preset.key}
                  variant="outline"
                  size="sm"
                  onClick={() => applyPreset(preset)}
                >
                  {preset.name}
                </Button>
              ))}
            </div>
          </div>

          <label className="flex flex-col gap-1 text-xs font-medium text-muted-foreground">
            Name
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="e.g. Stage Left"
            />
          </label>

          <label className="flex flex-col gap-1 text-xs font-medium text-muted-foreground">
            Start address
            <Input
              type="number"
              min={1}
              max={512}
              value={address}
              onChange={(e) => setAddress(Number(e.target.value))}
            />
          </label>

          <div>
            <div className="mb-1 flex items-center justify-between">
              <p className="text-xs font-medium text-muted-foreground">
                Channels{channels.length > 0 ? ` (${address}-${end})` : ""}
              </p>
              <Button
                variant="ghost"
                size="sm"
                className="h-7 px-2 text-xs"
                onClick={() =>
                  setChannels((cs) => [
                    ...cs,
                    { role: "Generic", label: `CH ${cs.length + 1}` },
                  ])
                }
              >
                <Plus className="mr-1 size-3" /> Add
              </Button>
            </div>
            <div className="flex max-h-40 flex-col gap-1 overflow-auto">
              {channels.length === 0 && (
                <p className="text-xs text-muted-foreground">
                  Pick a preset or add channels.
                </p>
              )}
              {channels.map((channel, i) => (
                <div key={i} className="flex items-center gap-1">
                  <select
                    aria-label={`Channel ${i + 1} role`}
                    className="rounded border bg-background px-1 py-1.5 text-xs"
                    value={channel.role}
                    onChange={(e) =>
                      setChannel(i, { role: e.target.value as LuxLabelColor })
                    }
                  >
                    {ROLES.map((role) => (
                      <option key={role} value={role}>
                        {role}
                      </option>
                    ))}
                  </select>
                  <Input
                    className="h-7 text-xs"
                    value={channel.label}
                    onChange={(e) => setChannel(i, { label: e.target.value })}
                  />
                  <Button
                    variant="ghost"
                    size="icon"
                    className="size-7 shrink-0"
                    aria-label="Remove channel"
                    onClick={() =>
                      setChannels((cs) => cs.filter((_, j) => j !== i))
                    }
                  >
                    <X className="size-3" />
                  </Button>
                </div>
              ))}
            </div>
          </div>

          {channels.length > 0 && end > 512 && (
            <p className="text-xs text-destructive">
              Doesn&apos;t fit. Ends past channel 512.
            </p>
          )}

          <Button onClick={submit} disabled={!fits}>
            Add fixture
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  );
}
