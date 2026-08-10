import { createFileRoute } from "@tanstack/react-router";
import PlanDesk from "@/components/plan/plan-desk";

export const Route = createFileRoute("/plan")({
  component: PlanDesk,
});
