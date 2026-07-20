import { createFileRoute } from "@tanstack/react-router";
import SharedDeskView from "@/components/shared-desk";

export const Route = createFileRoute("/shared")({
  component: SharedDeskView,
});
