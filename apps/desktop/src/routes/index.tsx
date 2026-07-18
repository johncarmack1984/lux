import { createFileRoute } from "@tanstack/react-router";
import ButtonRow from "@/components/button-row";
import FixturesView from "@/components/fixtures/fixtures-view";

export const Route = createFileRoute("/")({
  component: Home,
});

function Home() {
  return (
    <div className="flex min-h-0 w-full flex-1 flex-col items-center">
      <ButtonRow />
      <FixturesView />
    </div>
  );
}
