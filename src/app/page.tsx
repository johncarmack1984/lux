import ControlGrid from "@/components/control-grid/control-grid";
import Greeting from "@/components/greeting";

export default function Home() {
  return (
    <main className="flex min-h-screen flex-col items-center justify-start gap-14 py-[7.5%] px-12">
      <Greeting />
      <ControlGrid />
    </main>
  );
}
