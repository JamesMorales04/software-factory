import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: "http://127.0.0.1:8471",
        changeOrigin: false,
      },
      "/native-api": {
        target: "http://127.0.0.1:8765",
        changeOrigin: false,
        rewrite: (path) => path.replace(/^\/native-api/, ""),
      },
    },
  },
  build: {
    outDir: "dist",
    sourcemap: false,
  },
});
