import { createFileRoute } from "@tanstack/react-router";
import ButtonRow from "@/components/button-row";
import ControlGrid from "@/components/control-grid/grid";

export const Route = createFileRoute("/universe")({
  component: Universe,
});

function Universe() {
  return (
    <div className="flex w-full max-w-3xl flex-col items-center">
      <ButtonRow />
      <ControlGrid />
    </div>
  );
}
