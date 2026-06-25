import { createRootRoute, Link, Outlet } from "@tanstack/react-router";
import { Toaster } from "@/components/ui/sonner";
import { Updater } from "@/components/updater";
import SetupSwitcher from "@/components/setup-switcher";
import AccountMenu from "@/components/account-menu";

export const Route = createRootRoute({
  component: RootLayout,
});

const navLink =
  "text-sm font-medium text-muted-foreground transition-colors hover:text-foreground [&.active]:text-foreground";

function RootLayout() {
  return (
    <>
      <div className="flex min-h-screen flex-col sm:px-12">
        <nav className="flex items-center gap-4 border-b border-border/60 px-5 py-3">
          <SetupSwitcher />
          <div className="h-5 w-px bg-border/60" />
          <Link to="/" activeOptions={{ exact: true }} className={navLink}>
            Fixtures
          </Link>
          <Link to="/universe" className={navLink}>
            Universe
          </Link>
          <div className="ml-auto">
            <AccountMenu />
          </div>
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
