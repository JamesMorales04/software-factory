import react from "@vitejs/plugin-react";
import { playwright } from "@vitest/browser-playwright";
import { defineConfig } from "vitest/config";

declare const process: { env: Record<string, string | undefined> };

/**
 * Browser-mode component tests for the small set of Office surfaces where
 * jsdom cannot prove the behaviour: focus, real layout/CSS, portals and
 * native browser APIs. This is deliberately a separate lane from the 366
 * jsdom tests; it is not a replacement for them.
 */
export default defineConfig({
  plugins: [react()],
  test: {
    include: ["src/**/*.browser.test.{ts,tsx}"],
    browser: {
      enabled: true,
      headless: true,
      provider: playwright({
        launchOptions: process.env.CHROMIUM_EXE
          ? { executablePath: process.env.CHROMIUM_EXE }
          : undefined,
      }),
      instances: [{ browser: "chromium" }],
    },
    css: true,
    testTimeout: 20_000,
  },
});
