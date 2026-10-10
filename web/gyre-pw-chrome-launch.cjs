// probe: can chrome-headless-shell launch at all in this sandbox with --no-sandbox?
const { chromium } = require('playwright-core');
(async () => {
  try {
    const browser = await chromium.launch({
      executablePath: '/usr/local/share/gyre-playwright/chromium_headless_shell-1208/chrome-headless-shell-linux64/chrome-headless-shell',
      args: ['--no-sandbox', '--disable-gpu', '--disable-dev-shm-usage'],
    });
    const page = await browser.newPage();
    await page.goto('http://localhost:2222/');
    console.log('TITLE:', await page.title());
    await page.screenshot({ path: '/tmp/gyre-pw-probe.png' });
    await browser.close();
    console.log('PROBE OK');
  } catch (e) {
    console.error('PROBE FAIL:', e.message);
    process.exit(1);
  }
})();
