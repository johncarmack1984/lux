import { createFileRoute } from "@tanstack/react-router";
import ButtonRow from "@/components/button-row";
import ControlGrid from "@/components/control-grid/grid";
import ScenesPanel from "@/components/scenes/scenes-panel";

export const Route = createFileRoute("/universe")({
  component: Universe,
});

function Universe() {
  return (
    // This view fits the viewport — the desk scrolls internally, the page
    // never does. The presets row and the scenes panel take their height; the
    // desk absorbs the rest.
    <div className="mx-auto flex h-full w-full max-w-3xl flex-col px-4">
      <ButtonRow />
      <ScenesPanel />
      <div className="mb-3 min-h-0 w-full flex-1">
        <ControlGrid />
      </div>
    </div>
  );
}
