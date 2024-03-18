import ButtonRow from "@/components/button-row";
import ControlGrid from "@/components/control-grid";
import Greeting from "@/components/greeting";

export default function Home() {
  return (
    <main className="flex min-h-screen flex-col items-center justify-between py-8 px-12">
      <Greeting />
      <ButtonRow />
      <ControlGrid />
    </main>
  );
}
