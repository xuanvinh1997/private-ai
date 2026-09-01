import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
import tailwindcss from "@tailwindcss/vite";

// Tauri phục vụ giao diện qua một origin cố định, nên cổng phải cố định và
// `strictPort` phải bật: một cổng trượt là một cửa sổ trắng, không phải một cảnh báo.
export default defineConfig({
  plugins: [solid(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/target/**"] },
  },
  build: {
    target: "safari15",
    sourcemap: process.env.TAURI_ENV_DEBUG === "true",
    minify: process.env.TAURI_ENV_DEBUG === "true" ? false : "esbuild",
  },
});
