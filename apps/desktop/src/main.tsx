import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { createRouter, RouterProvider } from "@tanstack/react-router";
import { QueryClientProvider } from "@tanstack/react-query";
import { attachConsole } from "@tauri-apps/plugin-log";
import { queryClient } from "@/lib/query-client";
import { routeTree } from "./routeTree.gen";
import "@fontsource-variable/inter";
import "./globals.css";

// Pipe the Rust logger into the webview console (once, app-wide).
attachConsole();

const router = createRouter({ routeTree });

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

const rootElement = document.getElementById("root")!;
createRoot(rootElement).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  </StrictMode>
);
