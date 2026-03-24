import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests/e2e',
  use: {
    baseURL: 'http://localhost:2222',
  },
  screenshot: 'only-on-failure',
  timeout: 30_000,
  expect: { timeout: 10_000 },
  webServer: {
    // In CI the release binary is pre-built by the workflow; use it directly to avoid
    // recompiling inside the 120s timeout.  Locally fall back to `cargo run`.
    command: process.env.CI
      ? '../target/release/gyre-server'
      : 'cargo run -p gyre-server',
    url: 'http://localhost:2222/health',
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
    env: {
      GYRE_PORT: '2222',
      GYRE_AUTH_TOKEN: 'e2e-test-token',
    },
  },
  outputDir: 'test-results',
  reporter: process.env.CI ? 'github' : 'list',
});
