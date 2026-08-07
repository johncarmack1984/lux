import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import {
  ChevronLeft,
  ChevronRight,
  Camera,
  Pencil,
  Trash2,
} from "lucide-react";
import { type Scene } from "@/bindings";
import useBuffer from "@/hooks/useBuffer";
import useScenes from "@/hooks/useScenes";
import { activeSceneId } from "@/lib/scene-state";
import {
  captureScene,
  deleteScene,
  moveScene,
  recallScene,
  renameScene,
  setSceneFade,
  updateScene,
} from "@/lib/scene-actions";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";

/** The fade times a volunteer actually picks between, in milliseconds. */
const FADES = [
  { ms: 0, label: "Snap" },
  { ms: 1000, label: "1s" },
  { ms: 2000, label: "2s" },
  { ms: 3000, label: "3s" },
  { ms: 5000, label: "5s" },
  { ms: 10000, label: "10s" },
];

/** "2s" for the badge under a scene's name. */
function fadeLabel(fadeMs: number): string {
  return FADES.find((f) => f.ms === fadeMs)?.label ?? `${fadeMs / 1000}s`;
}

/**
 * One scene: a big tappable recall button with its editor tucked behind a
 * pencil. Recall is one tap and nothing else on this card can be hit by
 * accident — the editor is a separate, smaller target, and delete lives two
 * taps deep inside it.
 */
function SceneButton({
  scene,
  showing,
  disabled,
}: {
  scene: Scene;
  showing: boolean;
  disabled: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState(scene.name);
  // Seeded from the prop, never from a background refresh mid-edit — the same
  // rule the setup switcher's draft follows.
  useEffect(() => setDraft(scene.name), [scene.name]);

  // Deleting a scene has no undo, so the trash arms on the first tap and only
  // deletes on the second; it disarms on a timer or when the popover closes
  // (the setup trash's behaviour, deliberately mirrored).
  const [armed, setArmed] = useState(false);
  const disarm = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(
    () => () => {
      if (disarm.current) clearTimeout(disarm.current);
    },
    [],
  );

  const fail = (e: unknown) => toast.error(String(e));

  const commitName = () => {
    const next = draft.trim();
    if (!next || next === scene.name) {
      setDraft(scene.name);
      return;
    }
    renameScene(scene.id, next).catch(fail);
  };

  const onTrash = () => {
    if (armed) {
      setArmed(false);
      setOpen(false);
      deleteScene(scene.id).catch(fail);
      return;
    }
    setArmed(true);
    if (disarm.current) clearTimeout(disarm.current);
    disarm.current = setTimeout(() => setArmed(false), 3000);
  };

  return (
    <div className="relative">
      <button
        type="button"
        disabled={disabled}
        aria-pressed={showing}
        onClick={() => recallScene(scene.id).catch(fail)}
        className={cn(
          // Volunteer-grade target: comfortably past 44px on a phone, and the
          // whole tile is the button.
          "flex h-20 w-36 flex-col items-start justify-end gap-0.5 rounded-xl border p-3 text-left transition-colors",
          "disabled:pointer-events-none disabled:opacity-50",
          showing
            ? "border-primary bg-primary/15 text-foreground"
            : "border-input bg-background hover:bg-accent",
        )}
      >
        <span className="line-clamp-2 w-full text-sm font-medium leading-tight">
          {scene.name}
        </span>
        <span className="text-xs text-muted-foreground">
          {fadeLabel(scene.fadeMs)}
        </span>
      </button>

      <Popover
        open={open}
        onOpenChange={(next) => {
          setOpen(next);
          if (!next) setArmed(false);
        }}
      >
        <PopoverTrigger asChild>
          <Button
            variant="ghost"
            size="icon"
            aria-label={`Edit ${scene.name}`}
            className="absolute right-1 top-1 size-7 text-muted-foreground hover:text-foreground"
          >
            <Pencil className="size-3.5" />
          </Button>
        </PopoverTrigger>
        <PopoverContent align="end" className="flex w-64 flex-col gap-3 p-3">
          <Input
            autoFocus
            value={draft}
            aria-label="Scene name"
            className="h-8"
            onChange={(e) => setDraft(e.target.value)}
            onBlur={commitName}
            onKeyDown={(e) => {
              if (e.key === "Enter") commitName();
              if (e.key === "Escape") setDraft(scene.name);
            }}
          />

          <div className="flex flex-col gap-1.5">
            <p className="text-xs font-medium text-muted-foreground">
              Fade time
            </p>
            <div className="flex flex-wrap gap-1">
              {FADES.map((fade) => (
                <Button
                  key={fade.ms}
                  variant={scene.fadeMs === fade.ms ? "secondary" : "outline"}
                  size="sm"
                  className="h-7 px-2 text-xs"
                  onClick={() => setSceneFade(scene.id, fade.ms).catch(fail)}
                >
                  {fade.label}
                </Button>
              ))}
            </div>
          </div>

          <Button
            variant="outline"
            size="sm"
            className="justify-start gap-2"
            onClick={() => updateScene(scene.id).catch(fail)}
          >
            <Camera className="size-3.5" />
            Save current levels here
          </Button>

          <div className="flex items-center justify-between">
            <div className="flex gap-1">
              <Button
                variant="ghost"
                size="icon"
                className="size-7"
                aria-label={`Move ${scene.name} earlier`}
                onClick={() => moveScene(scene.id, -1).catch(fail)}
              >
                <ChevronLeft className="size-4" />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="size-7"
                aria-label={`Move ${scene.name} later`}
                onClick={() => moveScene(scene.id, 1).catch(fail)}
              >
                <ChevronRight className="size-4" />
              </Button>
            </div>
            <Button
              variant="ghost"
              size="sm"
              className={cn(
                "gap-1.5",
                armed
                  ? "bg-destructive/15 text-destructive hover:text-destructive"
                  : "text-muted-foreground hover:text-destructive",
              )}
              onClick={onTrash}
            >
              <Trash2 className="size-3.5" />
              {armed ? "Tap again" : "Delete"}
            </Button>
          </div>
        </PopoverContent>
      </Popover>
    </div>
  );
}

/**
 * The scenes panel: the saved looks for the active setup, plus the one button
 * that makes a new one.
 *
 * Recall is a single tap on a big target — the whole point of the brick. The
 * pressed scene is derived from the live buffer (`lib/scene-state`), not
 * remembered, so a scene shows as up whether it was recalled here, from another
 * device, or reproduced by hand on the faders.
 */
export default function ScenesPanel() {
  const scenes = useScenes();
  const buffer = useBuffer();
  const showing = activeSceneId(buffer, scenes);
  const [saving, setSaving] = useState(false);

  const save = async () => {
    setSaving(true);
    try {
      // A default name the user can immediately edit beats a modal in the way
      // of "save this, we're about to start".
      await captureScene(`Scene ${(scenes?.length ?? 0) + 1}`);
    } catch (e) {
      toast.error(String(e));
    }
    setSaving(false);
  };

  return (
    <div className="flex w-full max-w-2xl shrink-0 flex-col gap-2 px-4 pt-2">
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">Scenes</p>
        <Button
          variant="outline"
          size="sm"
          className="gap-1.5"
          disabled={saving || buffer === null}
          onClick={() => void save()}
        >
          <Camera className="size-3.5" />
          Save look
        </Button>
      </div>

      {scenes !== null &&
        (scenes.length === 0 ? (
          <div className="rounded-xl border border-dashed py-6 text-center text-sm text-muted-foreground">
            No scenes yet. Set a look, then Save look.
          </div>
        ) : (
          <div className="flex flex-wrap gap-2">
            {scenes.map((scene) => (
              <SceneButton
                key={scene.id}
                scene={scene}
                showing={scene.id === showing}
                disabled={buffer === null}
              />
            ))}
          </div>
        ))}
    </div>
  );
}
