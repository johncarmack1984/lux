import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import tailwindcss from "@tailwindcss/vite";

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    tanstackRouter({ target: "react", autoCodeSplitting: true }),
    react(),
    tailwindcss(),
  ],
  // Vite 8 resolves tsconfig `paths` (the @/* alias) natively.
  resolve: { tsconfigPaths: true },
  // Tauri controls the window; keep its logs visible and don't watch the Rust side.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    // Tauri renders in WKWebView on macOS; target a matching baseline.
    target: "safari15",
  },
});
