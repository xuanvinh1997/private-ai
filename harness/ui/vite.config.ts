import { defineConfig } from "vitest/config";
import solid from "vite-plugin-solid";
import tailwindcss from "@tailwindcss/vite";

// Tauri serves the UI from a fixed origin, so the port must be fixed and `strictPort` on.
export default defineConfig({
  plugins: [solid(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/target/**"] },
  },
  // `vite-plugin-solid` defaults to jsdom under vitest; `lib/` tests are pure logic, so force node.
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
  build: {
    target: "safari15",
    sourcemap: process.env.TAURI_ENV_DEBUG === "true",
    minify: process.env.TAURI_ENV_DEBUG === "true" ? false : "esbuild",
  },
});
