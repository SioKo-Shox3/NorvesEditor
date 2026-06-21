import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Vite config tuned for Tauri 2. See https://v2.tauri.app/start/frontend/vite/
export default defineConfig({
  plugins: [react()],
  // Prevent Vite from obscuring Rust errors.
  clearScreen: false,
  server: {
    // Tauri expects a fixed port; fail if it is taken.
    port: 1420,
    strictPort: true,
    host: false,
  },
  // Env vars starting with these prefixes are exposed to the frontend.
  envPrefix: ["VITE_", "TAURI_ENV_"],
  // Pre-bundle known deps to skip auto-discovery scan on dev cold start.
  optimizeDeps: {
    include: [
      "react",
      "react-dom/client",
      "dockview-react",
      "@tauri-apps/api/core",
      "@tauri-apps/api/event",
      "@tauri-apps/api/webviewWindow",
      "@tauri-apps/api/window",
    ],
  },
  build: {
    target: "esnext",
    outDir: "dist",
    // Produce readable errors during the build gate.
    sourcemap: false,
    rollupOptions: {
      output: {
        // Split vendor chunks to resolve 500 kB warning and improve cache reuse.
        // dockview-core is not directly resolvable as a bare specifier; use
        // id-based matching to capture the entire dockview family.
        manualChunks(id) {
          if (!id.includes("node_modules")) return undefined;
          if (id.includes("dockview")) return "dockview";
          if (
            id.includes("react-dom") ||
            id.includes("/react/") ||
            id.includes("scheduler")
          )
            return "react";
          return undefined;
        },
      },
    },
  },
});
