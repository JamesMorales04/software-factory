import { realpathSync } from "node:fs";
import { join } from "node:path";

import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

declare const process: { cwd(): string; env: Record<string, string | undefined> };

function nodeModulesAllow() {
  try {
    // A worktree may symlink node_modules to a shared install. Vite resolves
    // dependency files to their real path, so the dev server must allow it.
    return [realpathSync(join(process.cwd(), "node_modules"))];
  } catch {
    return [];
  }
}

export default defineConfig({
  plugins: [react()],
  server: {
    fs: { allow: [process.cwd(), ...nodeModulesAllow()] },
    port: 5173,
    proxy: {
      "/core-api": {
        target: process.env.FACTORY_CORE_URL || "http://127.0.0.1:8765",
        changeOrigin: false,
        rewrite: (path) => path.replace(/^\/core-api/, ""),
      },
    },
  },
  build: {
    outDir: "dist",
    sourcemap: false,
  },
});
