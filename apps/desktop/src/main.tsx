import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { createRouter, RouterProvider } from "@tanstack/react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { routeTree } from "./routeTree.gen";
import "@fontsource-variable/inter";
import "./globals.css";

const router = createRouter({ routeTree });

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

// Backend->frontend events aren't reliably delivered to the webview on iOS, so
// reactive state is read through TanStack Query (refetch on focus + invalidate
// after mutations) rather than relying on those events alone.
const queryClient = new QueryClient();

const rootElement = document.getElementById("root")!;
createRoot(rootElement).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  </StrictMode>
);
