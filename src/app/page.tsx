import ControlGrid from "@/components/control-grid/control-grid";
import Greeting from "@/components/greeting";

export default function Home() {
  return (
    <main className="flex min-h-screen flex-col items-center justify-start py-[7.5%] sm:px-12">
      <Greeting />
      <ControlGrid />
    </main>
  );
}
