import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const host = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],

  // Tauri 需要固定端口，且失败时不要回退到其它端口
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 不要让 Vite 监听 Rust 端源码
      ignored: ["**/src-tauri/**"],
    },
  },

  // Tauri 支持的浏览器目标
  build: {
    target: "chrome105",
    minify: "esbuild",
    sourcemap: false,
  },
}));
