import { createFileRoute } from "@tanstack/react-router";
import FixturesView from "@/components/fixtures/fixtures-view";

export const Route = createFileRoute("/")({
  component: Home,
});

function Home() {
  return <FixturesView />;
}
