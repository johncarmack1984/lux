import { createRootRoute, Outlet } from "@tanstack/react-router";
import { Toaster } from "@/components/ui/sonner";
import { Updater } from "@/components/updater";

export const Route = createRootRoute({
  component: RootLayout,
});

function RootLayout() {
  return (
    <>
      <div className="min-h-screen flex flex-col justify-center sm:px-12">
        <Outlet />
      </div>
      <Toaster closeButton />
      <Updater />
    </>
  );
}
