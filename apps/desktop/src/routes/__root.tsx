import { createRootRoute, Outlet } from "@tanstack/react-router";
import { Toaster } from "@/components/ui/sonner";
import { Updater } from "@/components/updater";
import SetupSwitcher from "@/components/setup-switcher";
import ViewMenu from "@/components/view-menu";
import AccountMenu from "@/components/account-menu";
import SettingsMenu from "@/components/settings-menu";
import SyncIndicator from "@/components/sync-indicator";
import RemoteIndicator from "@/components/remote-indicator";
import useSyncOnFocus from "@/hooks/useSyncOnFocus";

export const Route = createRootRoute({
  component: RootLayout,
});

function RootLayout() {
  useSyncOnFocus();
  return (
    <>
      <div className="flex h-[100dvh] flex-col sm:px-12">
        <nav className="flex shrink-0 items-center gap-4 border-b border-border/60 px-5 py-3">
          <SetupSwitcher />
          <div className="h-5 w-px bg-border/60" />
          <ViewMenu />
          <div className="ml-auto flex items-center gap-3">
            <RemoteIndicator />
            <SyncIndicator />
            <SettingsMenu />
            <AccountMenu />
          </div>
        </nav>
        {/* Routes own their scrolling: the fixtures list scrolls as a page,
            the universe desk fits the viewport and scrolls internally — a
            shared scroller can't serve both. min-h-0 lets flex children
            actually shrink to fit. */}
        <div className="flex min-h-0 flex-1 flex-col">
          <Outlet />
        </div>
        <footer className="shrink-0 pb-2 pt-1 text-center text-xs text-muted-foreground/70">
          lux v{__APP_VERSION__}
        </footer>
      </div>
      <Toaster closeButton />
      <Updater />
    </>
  );
}
