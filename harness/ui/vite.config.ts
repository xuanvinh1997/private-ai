import { defineConfig } from "vitest/config";
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
  // `vite-plugin-solid` tự đặt môi trường `jsdom` khi thấy vitest. Bài kiểm trong `lib/`
  // là logic thuần — nói rõ `node` để khỏi kéo thêm một phụ thuộc chỉ để chạy vài hàm
  // không chạm DOM. Thêm bài kiểm cho component thì đây là chỗ đổi.
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
