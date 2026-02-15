import { defineConfig } from '@playwright/test';

const appPort = Number(process.env.E2E_APP_PORT ?? 4173);

export default defineConfig({
  testDir: './e2e/tests',
  timeout: 90_000,
  expect: {
    timeout: 15_000,
  },
  fullyParallel: false,
  workers: 1,
  reporter: [['list'], ['html', { open: 'never' }]],
  globalSetup: './e2e/global-setup.mjs',
  use: {
    baseURL: `http://127.0.0.1:${appPort}`,
    viewport: { width: 1440, height: 900 },
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
  },
});
