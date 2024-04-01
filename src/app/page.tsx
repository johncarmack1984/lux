import ButtonRow from "@/components/button-row";
import ControlGrid from "@/components/control-grid/grid";
import Greeting from "@/components/greeting";
import BufferState from "./buffer-state";

export default function Home() {
  return (
    <>
      <Greeting />
      <ButtonRow />
      <ControlGrid />
    </>
  );
}
