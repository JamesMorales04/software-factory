import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  timeout: 30_000,
  fullyParallel: false,
  workers: 1,
  reporter: [['list']],
  use: {
    ...devices['Desktop Chrome'],
    headless: true,
    // Rootless Podman runs the container as (mapped) root, so Chromium's
    // setuid sandbox is unavailable. The spike measures the browser itself;
    // it does not claim a sandboxed browser boundary.
    launchOptions: { args: ['--no-sandbox'] },
  },
});
