import { defineConfig } from "vite";
import react, { reactCompilerPreset } from "@vitejs/plugin-react";
import babel from "@rolldown/plugin-babel";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import tailwindcss from "@tailwindcss/vite";
import pkg from "./package.json";

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    tanstackRouter({ target: "react", autoCodeSplitting: true }),
    react(), babel({ presets: [reactCompilerPreset()] }),
    tailwindcss(),
  ],
  // The release-please-stamped version (kept in lockstep with tauri.conf.json),
  // baked in at build time: no IPC, so it renders even where the window
  // capability excludes core:app (iOS).
  define: { __APP_VERSION__: JSON.stringify(pkg.version) },
  // Vite 8 resolves tsconfig `paths` (the @/* alias) natively.
  resolve: { tsconfigPaths: true },
  // Tauri controls the window; keep its logs visible and don't watch the Rust side.
  clearScreen: false,
  server: {
    // Non-default port so lux doesn't collide with other local Tauri/Vite apps.
    // Must match src-tauri/tauri.conf.json -> build.devUrl.
    port: 1430,
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
