import { defineConfig } from '@playwright/test';

const appPort = Number(process.env.E2E_APP_PORT ?? 4173);

export default defineConfig({
  testDir: './e2e/tests',
  timeout: 90_000,
  retries: 1,
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
    launchOptions: {
      args: ['--disable-dev-shm-usage'],
    },
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    video: 'on-first-retry',
  },
});
