import { createRootRoute, Link, Outlet } from "@tanstack/react-router";
import { Toaster } from "@/components/ui/sonner";
import { Updater } from "@/components/updater";
import SetupSwitcher from "@/components/setup-switcher";
import AccountMenu from "@/components/account-menu";
import SyncIndicator from "@/components/sync-indicator";
import useSyncOnFocus from "@/hooks/useSyncOnFocus";

export const Route = createRootRoute({
  component: RootLayout,
});

const navLink =
  "text-sm font-medium text-muted-foreground transition-colors hover:text-foreground [&.active]:text-foreground";

function RootLayout() {
  useSyncOnFocus();
  return (
    <>
      <div className="flex h-[100dvh] flex-col sm:px-12">
        <nav className="flex shrink-0 items-center gap-4 border-b border-border/60 px-5 py-3">
          <SetupSwitcher />
          <div className="h-5 w-px bg-border/60" />
          <Link to="/" activeOptions={{ exact: true }} className={navLink}>
            Fixtures
          </Link>
          <Link to="/universe" className={navLink}>
            Universe
          </Link>
          <div className="ml-auto flex items-center gap-3">
            <SyncIndicator />
            <AccountMenu />
          </div>
        </nav>
        <div className="flex flex-1 flex-col items-center overflow-y-auto">
          <Outlet />
          {/* mt-auto: sits at the viewport bottom until the content is tall
              enough to scroll, then trails it. */}
          <footer className="mt-auto pb-3 pt-8 text-xs text-muted-foreground/70">
            lux v{__APP_VERSION__}
          </footer>
        </div>
      </div>
      <Toaster closeButton />
      <Updater />
    </>
  );
}
