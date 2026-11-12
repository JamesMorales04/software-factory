import { realpathSync } from "node:fs";
import { join } from "node:path";

import react from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vite";

declare const process: {
  cwd(): string;
  env: Record<string, string | undefined>;
};

function nodeModulesAllow() {
  try {
    // A worktree may symlink node_modules to a shared install. Vite resolves
    // dependency files to their real path, so the dev server must allow it.
    return [realpathSync(join(process.cwd(), "node_modules"))];
  } catch {
    return [];
  }
}

function a11yProbe(): Plugin {
  return {
    name: "factory-a11y-probe",
    configureServer(server) {
      server.middlewares.use("/__factory_a11y_probe", (_request, response) => {
        response.setHeader("content-type", "text/plain");
        response.end(process.env.FACTORY_A11Y_PROBE || "unset");
      });
    },
  };
}

export default defineConfig({
  plugins: [react(), a11yProbe()],
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
