import { createFileRoute } from "@tanstack/react-router";
import ButtonRow from "@/components/button-row";
import ControlGrid from "@/components/control-grid/grid";
import Greeting from "@/components/greeting";
import ColorPicker from "@/components/color-picker/picker";

export const Route = createFileRoute("/")({
  component: Home,
});

function Home() {
  return (
    <div className="flex flex-col max-w-3xl w-full mx-auto items-center justify-start">
      <ColorPicker className="absolute top-3 right-5" />
      <Greeting />
      <ButtonRow />
      <ControlGrid />
    </div>
  );
}
