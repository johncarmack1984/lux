import { createRootRoute, Link, Outlet } from "@tanstack/react-router";
import { Toaster } from "@/components/ui/sonner";
import { Updater } from "@/components/updater";

export const Route = createRootRoute({
  component: RootLayout,
});

const navLink =
  "text-sm font-medium text-muted-foreground transition-colors hover:text-foreground [&.active]:text-foreground";

function RootLayout() {
  return (
    <>
      <div className="flex min-h-screen flex-col sm:px-12">
        <nav className="flex items-center gap-5 border-b border-border/60 px-5 py-3">
          <Link to="/" activeOptions={{ exact: true }} className={navLink}>
            Fixtures
          </Link>
          <Link to="/universe" className={navLink}>
            Universe
          </Link>
        </nav>
        <div className="flex flex-1 flex-col items-center">
          <Outlet />
        </div>
      </div>
      <Toaster closeButton />
      <Updater />
    </>
  );
}
