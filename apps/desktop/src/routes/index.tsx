import { createFileRoute } from "@tanstack/react-router";
import ButtonRow from "@/components/button-row";
import FixturesView from "@/components/fixtures/fixtures-view";

export const Route = createFileRoute("/")({
  component: Home,
});

function Home() {
  return (
    // This view scrolls as a page (many fixtures); min-h-full keeps the
    // vertical console able to fill the viewport when content is short.
    <div className="h-full w-full overflow-y-auto">
      <div className="flex min-h-full w-full flex-col items-center">
        <ButtonRow />
        <FixturesView />
      </div>
    </div>
  );
}
