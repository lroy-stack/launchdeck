import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed dev port; see src-tauri/tauri.conf.json.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  // The dev server watches the interface's sources. Rust's build output and
  // what the desktop checks write are not that, and are large and busy.
  server: { port: 1420, strictPort: true, host: "127.0.0.1", watch: { ignored: ["**/src-tauri/**", "**/tools/desktop-e2e/.work/**"] } },
  build: { target: "es2022", sourcemap: false },
  test: { environment: "node", include: ["src/**/*.test.{ts,tsx}"] },
});
