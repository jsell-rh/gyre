// Playwright config for in-sandbox visual verification of explorer-visual.spec.js.
// The repo's playwright.config.js launches gyre-server (cargo run) — impossible here:
// TCP listener probes are unsupported in this sandbox (errno 95). Instead a static
// SPA server (node /tmp/gyre-e2e-static-server.mjs) serves web/dist on :2222, and the
// tests' own page.route interception supplies all graph data. The seeded fixture's
// native-fetch bootstrap calls (/api/v1/admin/seed, /api/v1/workspaces) are answered
// by the static server itself.
// Chromium in this sandbox additionally needs --no-zygote (zygote fork: EPERM).
//
// FONT TRUTH: CI (ubuntu-latest, network open) loads the Red Hat webfonts from
// fonts.googleapis.com. This sandbox blocks that host, and chrome-headless-shell here
// ignores fontconfig-installed system fonts — only data-URL @font-face works. The
// spec file's beforeEach route hook (tests/e2e/explorer-visual.spec.js) rewrites the
// Google Fonts CSS request to /tmp/google-fonts-replacement.css, which embeds the
// same Red Hat woff2 faces (@fontsource/red-hat-*, same typeface Google serves) as
// data URLs. Without this, text metrics differ (CJK default sans → +6px toolbar,
// +4px canvas areas) and every screenshot comparison is meaningless.
import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests/e2e',
  testMatch: /explorer-visual\.spec\.js/,
  use: {
    baseURL: 'http://localhost:2222',
    launchOptions: {
      executablePath: process.env.PLAYWRIGHT_CHROMIUM_PATH,
      args: ['--no-sandbox', '--disable-gpu', '--no-zygote', '--disable-dev-shm-usage'],
    },
  },
  screenshot: 'only-on-failure',
  timeout: 30_000,
  expect: {
    timeout: 10_000,
    toHaveScreenshot: {
      maxDiffPixelRatio: 0.02,
    },
  },
  outputDir: '/tmp/gyre-e2e-test-results',
  reporter: 'list',
});
