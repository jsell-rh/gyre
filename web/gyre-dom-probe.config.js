import { defineConfig } from '@playwright/test';
export default defineConfig({
  testDir: '.',
  testMatch: /gyre-dom-probe\.spec\.js/,
  use: {
    baseURL: 'http://localhost:2222',
    launchOptions: {
      executablePath: process.env.PLAYWRIGHT_CHROMIUM_PATH,
      args: ['--no-sandbox', '--disable-gpu', '--no-zygote', '--disable-dev-shm-usage'],
    },
  },
  timeout: 30_000,
  outputDir: '/tmp/gyre-dom-probe-results',
  reporter: 'list',
});
