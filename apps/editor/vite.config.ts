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
  build: {
    target: "esnext",
    outDir: "dist",
    // Produce readable errors during the build gate.
    sourcemap: false,
  },
});
