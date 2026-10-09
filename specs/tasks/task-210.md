---
title: "Repair verified failure on main cd1c5f044e49"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: ["503b0b46cbf4039f1cdc8927e4b56f136e983c37", "01a25587bea99ebc24bfcbd1bc89929730bcf53c", "ae2b070d953130409afb861cf037b7eddc556fcf", "1e22f9899db66045a7b39ebc1dda670bf72957c2"]
---

## Required behavior

A required delivery gate failed on upstream main before this candidate. Reproduce and repair the existing production defect or meaningful broken test setup. Do not implement the blocked feature in this task. Do not remove tests, weaken gates, or claim an environmental outage is a production fix. Run the failing probe and obtain independent review; integration reruns cloud gates and the full suites.

Base: `cd1c5f044e49407ba5b11823fe2433950042feff`
Environment fingerprint: `github-9e53b7de697e8ff2274978f9658b305f9fa6eaf8f8c91ac0f63e76cd6ee54d31`

## Baseline failure

```text
-testid="back-btn"]')
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      297 |
e2e	UNKNOWN STEP	      298 |     const backBtn = page.locator('[data-testid="back-btn"]');
e2e	UNKNOWN STEP	    > 299 |     await expect(backBtn).toBeVisible({ timeout: 5000 });
e2e	UNKNOWN STEP	          |                           ^
e2e	UNKNOWN STEP	      300 |     await backBtn.click();
e2e	UNKNOWN STEP	      301 |     await page.waitForLoadState('networkidle');
e2e	UNKNOWN STEP	      302 |
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/app.spec.js:299:27
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1035809Z ##[error]  12) tests/e2e/app.spec.js:308:3 › Repo mode › repo_header_renders_with_repo_name ─────────────────
e2e	UNKNOWN STEP	    Error: expect(locator).toBeVisible() failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('[data-testid="repo-header"]')
e2e	UNKNOWN STEP	    Expected: visible
e2e	UNKNOWN STEP	    Timeout: 5000ms
e2e	UNKNOWN STEP	    Error: element(s) not found
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toBeVisible" with timeout 5000ms
e2e	UNKNOWN STEP	      - waiting for locator('[data-testid="repo-header"]')
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      311 |
e2e	UNKNOWN STEP	      312 |     const repoHeader = page.locator('[data-testid="repo-header"]');
e2e	UNKNOWN STEP	    > 313 |     await expect(repoHeader).toBeVisible({ timeout: 5000 });
e2e	UNKNOWN STEP	          |                              ^
e2e	UNKNOWN STEP	      314 |
e2e	UNKNOWN STEP	      315 |     const repoNameEl = page.locator('[data-testid="repo-name"]');
e2e	UNKNOWN STEP	      316 |     await expect(repoNameEl).toBeVisible({ timeout: 3000 });
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/app.spec.js:313:30
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1042450Z ##[error]  13) tests/e2e/app.spec.js:319:3 › Repo mode › architecture_tab_renders_and_is_active ─────────────
e2e	UNKNOWN STEP	    Error: expect(locator).toHaveAttribute(expected) failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('[data-testid="repo-tab-bar"]').getByRole('tab', { name: 'Architecture' })
e2e	UNKNOWN STEP	    Expected: "true"
e2e	UNKNOWN STEP	    Timeout: 5000ms
e2e	UNKNOWN STEP	    Error: element(s) not found
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toHaveAttribute" with timeout 5000ms
e2e	UNKNOWN STEP	      - waiting for locator('[data-testid="repo-tab-bar"]').getByRole('tab', { name: 'Architecture' })
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      322 |
e2e	UNKNOWN STEP	      323 |     const archTab = page.locator('[data-testid="repo-tab-bar"]').getByRole('tab', { name: 'Architecture' });
e2e	UNKNOWN STEP	    > 324 |     await expect(archTab).toHaveAttribute('aria-selected', 'true', { timeout: 5000 });
e2e	UNKNOWN STEP	          |                           ^
e2e	UNKNOWN STEP	      325 |
e2e	UNKNOWN STEP	      326 |     // Tab panel content should be visible
e2e	UNKNOWN STEP	      327 |     const tabContent = page.locator('[role="tabpanel"]');
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/app.spec.js:324:27
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1049577Z ##[error]  14) tests/e2e/app.spec.js:331:3 › Repo mode › decisions_tab_renders_and_is_active ────────────────
e2e	UNKNOWN STEP	    Error: expect(locator).toHaveAttribute(expected) failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('[data-testid="repo-tab-bar"]').getByRole('tab', { name: 'Decisions' })
e2e	UNKNOWN STEP	    Expected: "true"
e2e	UNKNOWN STEP	    Timeout: 5000ms
e2e	UNKNOWN STEP	    Error: element(s) not found
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toHaveAttribute" with timeout 5000ms
e2e	UNKNOWN STEP	      - waiting for locator('[data-testid="repo-tab-bar"]').getByRole('tab', { name: 'Decisions' })
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      334 |
e2e	UNKNOWN STEP	      335 |     const decisionsTab = page.locator('[data-testid="repo-tab-bar"]').getByRole('tab', { name: 'Decisions' });
e2e	UNKNOWN STEP	    > 336 |     await expect(decisionsTab).toHaveAttribute('aria-selected', 'true', { timeout: 5000 });
e2e	UNKNOWN STEP	          |                                ^
e2e	UNKNOWN STEP	      337 |   });
e2e	UNKNOWN STEP	      338 |
e2e	UNKNOWN STEP	      339 |   test('specs_tab_renders_spec_list_or_empty_state', async ({ page }) => {
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/app.spec.js:336:32
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1055199Z ##[error]  15) tests/e2e/app.spec.js:339:3 › Repo mode › specs_tab_renders_spec_list_or_empty_state ─────────
e2e	UNKNOWN STEP	    Error: expect(locator).toBeVisible() failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('[role="tabpanel"]')
e2e	UNKNOWN STEP	    Expected: visible
e2e	UNKNOWN STEP	    Timeout: 5000ms
e2e	UNKNOWN STEP	    Error: element(s) not found
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toBeVisible" with timeout 5000ms
e2e	UNKNOWN STEP	      - waiting for locator('[role="tabpanel"]')
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      342 |
e2e	UNKNOWN STEP	      343 |     const tabContent = page.locator('[role="tabpanel"]');
e2e	UNKNOWN STEP	    > 344 |     await expect(tabContent).toBeVisible({ timeout: 5000 });
e2e	UNKNOWN STEP	          |                              ^
e2e	UNKNOWN STEP	      345 |
e2e	UNKNOWN STEP	      346 |     // Spec content or empty state should be visible inside the tab panel
e2e	UNKNOWN STEP	      347 |     const content = tabContent
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/app.spec.js:344:30
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1059678Z ##[error]  16) tests/e2e/app.spec.js:362:3 › URL routing › workspace_home_url_loads ─────────────────────────
e2e	UNKNOWN STEP	    Error: expect(received).toContain(expected) // indexOf
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Expected substring: "/workspaces/default"
e2e	UNKNOWN STEP	    Received string:    "http://localhost:2222/"
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      365 |
e2e	UNKNOWN STEP	      366 |     await expect(page.locator('.app')).toBeVisible({ timeout: 5000 });
e2e	UNKNOWN STEP	    > 367 |     expect(page.url()).toContain(`/workspaces/${SEED_SLUG}`);
e2e	UNKNOWN STEP	          |                        ^
e2e	UNKNOWN STEP	      368 |   });
e2e	UNKNOWN STEP	      369 |
e2e	UNKNOWN STEP	      370 |   test('repo_url_loads_with_tab_bar', async ({ page }) => {
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/app.spec.js:367:24
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1065790Z ##[error]  17) tests/e2e/app.spec.js:370:3 › URL routing › repo_url_loads_with_tab_bar ──────────────────────
e2e	UNKNOWN STEP	    Error: expect(locator).toBeVisible() failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('[data-testid="repo-tab-bar"]')
e2e	UNKNOWN STEP	    Expected: visible
e2e	UNKNOWN STEP	    Timeout: 5000ms
e2e	UNKNOWN STEP	    Error: element(s) not found
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toBeVisible" with timeout 5000ms
e2e	UNKNOWN STEP	      - waiting for locator('[data-testid="repo-tab-bar"]')
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      374 |     await expect(page.locator('.app')).toBeVisible({ timeout: 5000 });
e2e	UNKNOWN STEP	      375 |     const tabBar = page.locator('[data-testid="repo-tab-bar"]');
e2e	UNKNOWN STEP	    > 376 |     await expect(tabBar).toBeVisible({ timeout: 5000 });
e2e	UNKNOWN STEP	          |                          ^
e2e	UNKNOWN STEP	      377 |   });
e2e	UNKNOWN STEP	      378 |
e2e	UNKNOWN STEP	      379 |   test('repo_architecture_url_activates_architecture_tab', async ({ page }) => {
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/app.spec.js:376:26
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1072457Z ##[error]  18) tests/e2e/app.spec.js:379:3 › URL routing › repo_architecture_url_activates_architecture_tab ─
e2e	UNKNOWN STEP	    Error: expect(locator).toHaveAttribute(expected) failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('[data-testid="repo-tab-bar"]').getByRole('tab', { name: 'Architecture' })
e2e	UNKNOWN STEP	    Expected: "true"
e2e	UNKNOWN STEP	    Timeout: 5000ms
e2e	UNKNOWN STEP	    Error: element(s) not found
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toHaveAttribute" with timeout 5000ms
e2e	UNKNOWN STEP	      - waiting for locator('[data-testid="repo-tab-bar"]').getByRole('tab', { name: 'Architecture' })
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      382 |
e2e	UNKNOWN STEP	      383 |     const archTab = page.locator('[data-testid="repo-tab-bar"]').getByRole('tab', { name: 'Architecture' });
e2e	UNKNOWN STEP	    > 384 |     await expect(archTab).toHaveAttribute('aria-selected', 'true', { timeout: 5000 });
e2e	UNKNOWN STEP	          |                           ^
e2e	UNKNOWN STEP	      385 |   });
e2e	UNKNOWN STEP	      386 |
e2e	UNKNOWN STEP	      387 |   test('unknown_route_falls_back_gracefully', async ({ page }) => {
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/app.spec.js:384:27
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1078235Z ##[error]  19) tests/e2e/app.spec.js:394:3 › URL routing › browser_back_forward_navigation ──────────────────
e2e	UNKNOWN STEP	    Error: expect(locator).toBeVisible() failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('[data-testid="repo-mode"]')
e2e	UNKNOWN STEP	    Expected: visible
e2e	UNKNOWN STEP	    Timeout: 5000ms
e2e	UNKNOWN STEP	    Error: element(s) not found
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toBeVisible" with timeout 5000ms
e2e	UNKNOWN STEP	      - waiting for locator('[data-testid="repo-mode"]')
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      408 |     await page.goForward();
e2e	UNKNOWN STEP	      409 |     await page.waitForLoadState('networkidle');
e2e	UNKNOWN STEP	    > 410 |     await expect(page.locator('[data-testid="repo-mode"]')).toBeVisible({ timeout: 5000 });
e2e	UNKNOWN STEP	          |                                                             ^
e2e	UNKNOWN STEP	      411 |   });
e2e	UNKNOWN STEP	      412 |
e2e	UNKNOWN STEP	      413 |   test('profile_url_renders', async ({ page }) => {
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/app.spec.js:410:61
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1085055Z ##[error]  20) tests/e2e/app.spec.js:506:3 › Keyboard shortcuts › g_1_navigates_to_specs_tab_in_repo_mode ───
e2e	UNKNOWN STEP	    Error: expect(locator).toHaveAttribute(expected) failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('[data-testid="repo-tab-bar"]').getByRole('tab', { name: 'Specs' })
e2e	UNKNOWN STEP	    Expected: "true"
e2e	UNKNOWN STEP	    Timeout: 3000ms
e2e	UNKNOWN STEP	    Error: element(s) not found
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toHaveAttribute" with timeout 3000ms
e2e	UNKNOWN STEP	      - waiting for locator('[data-testid="repo-tab-bar"]').getByRole('tab', { name: 'Specs' })
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      515 |
e2e	UNKNOWN STEP	      516 |     const specsTab = page.locator('[data-testid="repo-tab-bar"]').getByRole('tab', { name: 'Specs' });
e2e	UNKNOWN STEP	    > 517 |     await expect(specsTab).toHaveAttribute('aria-selected', 'true', { timeout: 3000 });
e2e	UNKNOWN STEP	          |                            ^
e2e	UNKNOWN STEP	      518 |   });
e2e	UNKNOWN STEP	      519 |
e2e	UNKNOWN STEP	      520 |   test('g_2_navigates_to_architecture_tab_in_repo_mode', async ({ page }) => {
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/app.spec.js:517:28
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1091780Z ##[error]  21) tests/e2e/app.spec.js:520:3 › Keyboard shortcuts › g_2_navigates_to_architecture_tab_in_repo_mode 
e2e	UNKNOWN STEP	    Error: expect(locator).toHaveAttribute(expected) failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('[data-testid="repo-tab-bar"]').getByRole('tab', { name: 'Architecture' })
e2e	UNKNOWN STEP	    Expected: "true"
e2e	UNKNOWN STEP	    Timeout: 3000ms
e2e	UNKNOWN STEP	    Error: element(s) not found
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toHaveAttribute" with timeout 3000ms
e2e	UNKNOWN STEP	      - waiting for locator('[data-testid="repo-tab-bar"]').getByRole('tab', { name: 'Architecture' })
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      528 |
e2e	UNKNOWN STEP	      529 |     const archTab = page.locator('[data-testid="repo-tab-bar"]').getByRole('tab', { name: 'Architecture' });
e2e	UNKNOWN STEP	    > 530 |     await expect(archTab).toHaveAttribute('aria-selected', 'true', { timeout: 3000 });
e2e	UNKNOWN STEP	          |                           ^
e2e	UNKNOWN STEP	      531 |   });
e2e	UNKNOWN STEP	      532 |
e2e	UNKNOWN STEP	      533 |   test('esc_in_repo_mode_returns_to_workspace_home', async ({ page }) => {
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/app.spec.js:530:27
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1097805Z ##[error]  22) tests/e2e/app.spec.js:623:3 › Accessibility › repo_tab_bar_has_tablist_role ──────────────────
e2e	UNKNOWN STEP	    Error: expect(locator).toBeVisible() failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: getByRole('tablist', { name: /repo navigation/i })
e2e	UNKNOWN STEP	    Expected: visible
e2e	UNKNOWN STEP	    Timeout: 5000ms
e2e	UNKNOWN STEP	    Error: element(s) not found
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toBeVisible" with timeout 5000ms
e2e	UNKNOWN STEP	      - waiting for getByRole('tablist', { name: /repo navigation/i })
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      627 |     // Tab bar should have role=tablist with aria-label
e2e	UNKNOWN STEP	      628 |     const tabBar = page.getByRole('tablist', { name: /repo navigation/i });
e2e	UNKNOWN STEP	    > 629 |     await expect(tabBar).toBeVisible({ timeout: 5000 });
e2e	UNKNOWN STEP	          |                          ^
e2e	UNKNOWN STEP	      630 |   });
e2e	UNKNOWN STEP	      631 | });
e2e	UNKNOWN STEP	      632 |
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/app.spec.js:629:26
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1111194Z ##[error]  23) tests/e2e/explorer-visual.spec.js:308:3 › Semantic zoom visual regression › zoom_level_0_packages_overview 
e2e	UNKNOWN STEP	    Error: expect(locator).toHaveScreenshot(expected) failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	      Expected an image 919px by 395px, received 679px by 380px. 103122 pixels (ratio 0.29 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      Snapshot: zoom-level-0-packages.png
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toHaveScreenshot(zoom-level-0-packages.png)" with timeout 10000ms
e2e	UNKNOWN STEP	        - verifying given screenshot expectation
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-canvas-area svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - Expected an image 919px by 395px, received 679px by 380px. 103122 pixels (ratio 0.29 of all image pixels) are different.
e2e	UNKNOWN STEP	      - waiting 100ms before taking screenshot
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-canvas-area svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - captured a stable screenshot
e2e	UNKNOWN STEP	      - Expected an image 919px by 395px, received 679px by 380px. 103122 pixels (ratio 0.29 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      323 |
e2e	UNKNOWN STEP	      324 |     const canvasArea = page.locator('.treemap-canvas-area');
e2e	UNKNOWN STEP	    > 325 |     await expect(canvasArea).toHaveScreenshot('zoom-level-0-packages.png', {
e2e	UNKNOWN STEP	          |                              ^
e2e	UNKNOWN STEP	      326 |       maxDiffPixelRatio: 0.02,
e2e	UNKNOWN STEP	      327 |       timeout: 10_000,
e2e	UNKNOWN STEP	      328 |     });
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/explorer-visual.spec.js:325:30
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1124656Z ##[error]  24) tests/e2e/explorer-visual.spec.js:331:3 › Semantic zoom visual regression › zoom_level_1_modules 
e2e	UNKNOWN STEP	    Error: expect(locator).toHaveScreenshot(expected) failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	      Expected an image 919px by 395px, received 679px by 380px. 105394 pixels (ratio 0.30 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      Snapshot: zoom-level-1-modules.png
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toHaveScreenshot(zoom-level-1-modules.png)" with timeout 10000ms
e2e	UNKNOWN STEP	        - verifying given screenshot expectation
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-canvas-area svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - Expected an image 919px by 395px, received 679px by 380px. 105217 pixels (ratio 0.29 of all image pixels) are different.
e2e	UNKNOWN STEP	      - waiting 100ms before taking screenshot
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-canvas-area svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - captured a stable screenshot
e2e	UNKNOWN STEP	      - Expected an image 919px by 395px, received 679px by 380px. 105394 pixels (ratio 0.30 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      347 |
e2e	UNKNOWN STEP	      348 |     const canvasArea = page.locator('.treemap-canvas-area');
e2e	UNKNOWN STEP	    > 349 |     await expect(canvasArea).toHaveScreenshot('zoom-level-1-modules.png', {
e2e	UNKNOWN STEP	          |                              ^
e2e	UNKNOWN STEP	      350 |       maxDiffPixelRatio: 0.02,
e2e	UNKNOWN STEP	      351 |       timeout: 10_000,
e2e	UNKNOWN STEP	      352 |     });
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/explorer-visual.spec.js:349:30
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1138153Z ##[error]  25) tests/e2e/explorer-visual.spec.js:355:3 › Semantic zoom visual regression › zoom_level_2_types_and_functions 
e2e	UNKNOWN STEP	    Error: expect(locator).toHaveScreenshot(expected) failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	      Expected an image 919px by 395px, received 679px by 380px. 103007 pixels (ratio 0.29 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      Snapshot: zoom-level-2-types.png
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toHaveScreenshot(zoom-level-2-types.png)" with timeout 10000ms
e2e	UNKNOWN STEP	        - verifying given screenshot expectation
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-canvas-area svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - Expected an image 919px by 395px, received 679px by 380px. 103007 pixels (ratio 0.29 of all image pixels) are different.
e2e	UNKNOWN STEP	      - waiting 100ms before taking screenshot
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-canvas-area svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - captured a stable screenshot
e2e	UNKNOWN STEP	      - Expected an image 919px by 395px, received 679px by 380px. 103007 pixels (ratio 0.29 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      370 |
e2e	UNKNOWN STEP	      371 |     const canvasArea = page.locator('.treemap-canvas-area');
e2e	UNKNOWN STEP	    > 372 |     await expect(canvasArea).toHaveScreenshot('zoom-level-2-types.png', {
e2e	UNKNOWN STEP	          |                              ^
e2e	UNKNOWN STEP	      373 |       maxDiffPixelRatio: 0.02,
e2e	UNKNOWN STEP	      374 |       timeout: 10_000,
e2e	UNKNOWN STEP	      375 |     });
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/explorer-visual.spec.js:372:30
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1152047Z ##[error]  26) tests/e2e/explorer-visual.spec.js:388:3 › View query rendering visual regression › view_query_with_groups_callouts_narrative 
e2e	UNKNOWN STEP	    Error: expect(locator).toHaveScreenshot(expected) failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	      Expected an image 579px by 326px, received 339px by 276px. 91554 pixels (ratio 0.49 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      Snapshot: view-query-annotated.png
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toHaveScreenshot(view-query-annotated.png)" with timeout 10000ms
e2e	UNKNOWN STEP	        - verifying given screenshot expectation
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-canvas-area svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - Expected an image 579px by 326px, received 339px by 276px. 91554 pixels (ratio 0.49 of all image pixels) are different.
e2e	UNKNOWN STEP	      - waiting 100ms before taking screenshot
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-canvas-area svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - captured a stable screenshot
e2e	UNKNOWN STEP	      - Expected an image 579px by 326px, received 339px by 276px. 91554 pixels (ratio 0.49 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      395 |     // groups, callouts, and narrative markers should be visible
e2e	UNKNOWN STEP	      396 |     const canvasArea = page.locator('.treemap-canvas-area');
e2e	UNKNOWN STEP	    > 397 |     await expect(canvasArea).toHaveScreenshot('view-query-annotated.png', {
e2e	UNKNOWN STEP	          |                              ^
e2e	UNKNOWN STEP	      398 |       maxDiffPixelRatio: 0.02,
e2e	UNKNOWN STEP	      399 |       timeout: 10_000,
e2e	UNKNOWN STEP	      400 |     });
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/explorer-visual.spec.js:397:30
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1166068Z ##[error]  27) tests/e2e/explorer-visual.spec.js:403:3 › View query rendering visual regression › view_query_container_with_annotation_bar 
e2e	UNKNOWN STEP	    Error: expect(locator).toHaveScreenshot(expected) failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('.treemap-container')
e2e	UNKNOWN STEP	      Expected an image 579px by 468px, received 339px by 468px. 112369 pixels (ratio 0.42 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      Snapshot: view-query-container.png
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toHaveScreenshot(view-query-container.png)" with timeout 10000ms
e2e	UNKNOWN STEP	        - verifying given screenshot expectation
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-container')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-container svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - Expected an image 579px by 468px, received 339px by 468px. 112369 pixels (ratio 0.42 of all image pixels) are different.
e2e	UNKNOWN STEP	      - waiting 100ms before taking screenshot
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-container')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-container svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - captured a stable screenshot
e2e	UNKNOWN STEP	      - Expected an image 579px by 468px, received 339px by 468px. 112369 pixels (ratio 0.42 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      409 |     // Capture the full container including toolbar and annotation bar
e2e	UNKNOWN STEP	      410 |     const container = page.locator('.treemap-container');
e2e	UNKNOWN STEP	    > 411 |     await expect(container).toHaveScreenshot('view-query-container.png', {
e2e	UNKNOWN STEP	          |                             ^
e2e	UNKNOWN STEP	      412 |       maxDiffPixelRatio: 0.02,
e2e	UNKNOWN STEP	      413 |       timeout: 10_000,
e2e	UNKNOWN STEP	      414 |     });
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/explorer-visual.spec.js:411:29
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1179657Z ##[error]  28) tests/e2e/explorer-visual.spec.js:427:3 › Filter presets visual regression › filter_all_shows_complete_graph 
e2e	UNKNOWN STEP	    Error: expect(locator).toHaveScreenshot(expected) failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	      Expected an image 919px by 395px, received 679px by 380px. 104614 pixels (ratio 0.29 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      Snapshot: filter-all.png
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toHaveScreenshot(filter-all.png)" with timeout 10000ms
e2e	UNKNOWN STEP	        - verifying given screenshot expectation
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-canvas-area svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - Expected an image 919px by 395px, received 679px by 380px. 104614 pixels (ratio 0.29 of all image pixels) are different.
e2e	UNKNOWN STEP	      - waiting 100ms before taking screenshot
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-canvas-area svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - captured a stable screenshot
e2e	UNKNOWN STEP	      - Expected an image 919px by 395px, received 679px by 380px. 104614 pixels (ratio 0.29 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      430 |     // Default filter is 'all' — all nodes visible, no query applied
e2e	UNKNOWN STEP	      431 |     const canvasArea = page.locator('.treemap-canvas-area');
e2e	UNKNOWN STEP	    > 432 |     await expect(canvasArea).toHaveScreenshot('filter-all.png', {
e2e	UNKNOWN STEP	          |                              ^
e2e	UNKNOWN STEP	      433 |       maxDiffPixelRatio: 0.02,
e2e	UNKNOWN STEP	      434 |       timeout: 10_000,
e2e	UNKNOWN STEP	      435 |     });
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/explorer-visual.spec.js:432:30
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1193151Z ##[error]  29) tests/e2e/explorer-visual.spec.js:438:3 › Filter presets visual regression › filter_endpoints_shows_only_endpoints 
e2e	UNKNOWN STEP	    Error: expect(locator).toHaveScreenshot(expected) failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	      Expected an image 579px by 363px, received 339px by 328px. 95150 pixels (ratio 0.46 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      Snapshot: filter-endpoints.png
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toHaveScreenshot(filter-endpoints.png)" with timeout 10000ms
e2e	UNKNOWN STEP	        - verifying given screenshot expectation
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-canvas-area svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - Expected an image 579px by 363px, received 339px by 328px. 95150 pixels (ratio 0.46 of all image pixels) are different.
e2e	UNKNOWN STEP	      - waiting 100ms before taking screenshot
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-canvas-area svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - captured a stable screenshot
e2e	UNKNOWN STEP	      - Expected an image 579px by 363px, received 339px by 328px. 95150 pixels (ratio 0.46 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      448 |
e2e	UNKNOWN STEP	      449 |     const canvasArea = page.locator('.treemap-canvas-area');
e2e	UNKNOWN STEP	    > 450 |     await expect(canvasArea).toHaveScreenshot('filter-endpoints.png', {
e2e	UNKNOWN STEP	          |                              ^
e2e	UNKNOWN STEP	      451 |       maxDiffPixelRatio: 0.02,
e2e	UNKNOWN STEP	      452 |       timeout: 10_000,
e2e	UNKNOWN STEP	      453 |     });
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/explorer-visual.spec.js:450:30
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1206303Z ##[error]  30) tests/e2e/explorer-visual.spec.js:456:3 › Filter presets visual regression › filter_types_shows_only_type_nodes 
e2e	UNKNOWN STEP	    Error: expect(locator).toHaveScreenshot(expected) failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	      Expected an image 579px by 363px, received 339px by 328px. 96425 pixels (ratio 0.46 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      Snapshot: filter-types.png
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toHaveScreenshot(filter-types.png)" with timeout 10000ms
e2e	UNKNOWN STEP	        - verifying given screenshot expectation
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-canvas-area svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - Expected an image 579px by 363px, received 339px by 328px. 96417 pixels (ratio 0.46 of all image pixels) are different.
e2e	UNKNOWN STEP	      - waiting 100ms before taking screenshot
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-canvas-area svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - captured a stable screenshot
e2e	UNKNOWN STEP	      - Expected an image 579px by 363px, received 339px by 328px. 96425 pixels (ratio 0.46 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      479 |
e2e	UNKNOWN STEP	      480 |     const canvasArea = page.locator('.treemap-canvas-area');
e2e	UNKNOWN STEP	    > 481 |     await expect(canvasArea).toHaveScreenshot('filter-types.png', {
e2e	UNKNOWN STEP	          |                              ^
e2e	UNKNOWN STEP	      482 |       maxDiffPixelRatio: 0.02,
e2e	UNKNOWN STEP	      483 |       timeout: 10_000,
e2e	UNKNOWN STEP	      484 |     });
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/explorer-visual.spec.js:481:30
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1219422Z ##[error]  31) tests/e2e/explorer-visual.spec.js:487:3 › Filter presets visual regression › filter_calls_shows_call_graph 
e2e	UNKNOWN STEP	    Error: expect(locator).toHaveScreenshot(expected) failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	      Expected an image 579px by 363px, received 339px by 328px. 96092 pixels (ratio 0.46 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      Snapshot: filter-calls.png
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toHaveScreenshot(filter-calls.png)" with timeout 10000ms
e2e	UNKNOWN STEP	        - verifying given screenshot expectation
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-canvas-area svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - Expected an image 579px by 363px, received 339px by 328px. 96092 pixels (ratio 0.46 of all image pixels) are different.
e2e	UNKNOWN STEP	      - waiting 100ms before taking screenshot
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-canvas-area svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - captured a stable screenshot
e2e	UNKNOWN STEP	      - Expected an image 579px by 363px, received 339px by 328px. 96092 pixels (ratio 0.46 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      503 |
e2e	UNKNOWN STEP	      504 |     const canvasArea = page.locator('.treemap-canvas-area');
e2e	UNKNOWN STEP	    > 505 |     await expect(canvasArea).toHaveScreenshot('filter-calls.png', {
e2e	UNKNOWN STEP	          |                              ^
e2e	UNKNOWN STEP	      506 |       maxDiffPixelRatio: 0.02,
e2e	UNKNOWN STEP	      507 |       timeout: 10_000,
e2e	UNKNOWN STEP	      508 |     });
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/explorer-visual.spec.js:505:30
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1233872Z ##[error]  32) tests/e2e/explorer-visual.spec.js:511:3 › Filter presets visual regression › filter_dependencies_shows_dependency_edges 
e2e	UNKNOWN STEP	    Error: expect(locator).toHaveScreenshot(expected) failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	      Expected an image 579px by 363px, received 339px by 328px. 95896 pixels (ratio 0.46 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      Snapshot: filter-dependencies.png
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toHaveScreenshot(filter-dependencies.png)" with timeout 10000ms
e2e	UNKNOWN STEP	        - verifying given screenshot expectation
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-canvas-area svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - Expected an image 579px by 363px, received 339px by 328px. 95896 pixels (ratio 0.46 of all image pixels) are different.
e2e	UNKNOWN STEP	      - waiting 100ms before taking screenshot
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-canvas-area svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - captured a stable screenshot
e2e	UNKNOWN STEP	      - Expected an image 579px by 363px, received 339px by 328px. 95896 pixels (ratio 0.46 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      527 |
e2e	UNKNOWN STEP	      528 |     const canvasArea = page.locator('.treemap-canvas-area');
e2e	UNKNOWN STEP	    > 529 |     await expect(canvasArea).toHaveScreenshot('filter-dependencies.png', {
e2e	UNKNOWN STEP	          |                              ^
e2e	UNKNOWN STEP	      530 |       maxDiffPixelRatio: 0.02,
e2e	UNKNOWN STEP	      531 |       timeout: 10_000,
e2e	UNKNOWN STEP	      532 |     });
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/explorer-visual.spec.js:529:30
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1248245Z ##[error]  33) tests/e2e/explorer-visual.spec.js:545:3 › Blast radius visual regression › blast_radius_tiered_coloring_on_node_click 
e2e	UNKNOWN STEP	    Error: expect(locator).toHaveScreenshot(expected) failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	      Expected an image 259px by 238px, received 19px by 201px. 54864 pixels (ratio 0.90 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      Snapshot: blast-radius-tiered.png
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toHaveScreenshot(blast-radius-tiered.png)" with timeout 10000ms
e2e	UNKNOWN STEP	        - verifying given screenshot expectation
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-canvas-area svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - Expected an image 259px by 238px, received 19px by 201px. 54864 pixels (ratio 0.90 of all image pixels) are different.
e2e	UNKNOWN STEP	      - waiting 100ms before taking screenshot
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-canvas-area svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - captured a stable screenshot
e2e	UNKNOWN STEP	      - Expected an image 259px by 238px, received 19px by 201px. 54864 pixels (ratio 0.90 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      567 |     // tiered coloring (red → orange → yellow → gray) with dimmed unmatched nodes
e2e	UNKNOWN STEP	      568 |     const canvasArea = page.locator('.treemap-canvas-area');
e2e	UNKNOWN STEP	    > 569 |     await expect(canvasArea).toHaveScreenshot('blast-radius-tiered.png', {
e2e	UNKNOWN STEP	          |                              ^
e2e	UNKNOWN STEP	      570 |       maxDiffPixelRatio: 0.02,
e2e	UNKNOWN STEP	      571 |       timeout: 10_000,
e2e	UNKNOWN STEP	      572 |     });
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/explorer-visual.spec.js:569:30
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1261836Z ##[error]  34) tests/e2e/explorer-visual.spec.js:575:3 › Blast radius visual regression › blast_radius_dimmed_unmatched_nodes 
e2e	UNKNOWN STEP	    Error: expect(locator).toHaveScreenshot(expected) failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	      Expected an image 579px by 326px, received 339px by 276px. 92788 pixels (ratio 0.50 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      Snapshot: blast-radius-fixed-node.png
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toHaveScreenshot(blast-radius-fixed-node.png)" with timeout 10000ms
e2e	UNKNOWN STEP	        - verifying given screenshot expectation
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-canvas-area svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - Expected an image 579px by 326px, received 339px by 276px. 92788 pixels (ratio 0.50 of all image pixels) are different.
e2e	UNKNOWN STEP	      - waiting 100ms before taking screenshot
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-canvas-area')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-canvas-area svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - captured a stable screenshot
e2e	UNKNOWN STEP	      - Expected an image 579px by 326px, received 339px by 276px. 92788 pixels (ratio 0.50 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      600 |
e2e	UNKNOWN STEP	      601 |     const canvasArea = page.locator('.treemap-canvas-area');
e2e	UNKNOWN STEP	    > 602 |     await expect(canvasArea).toHaveScreenshot('blast-radius-fixed-node.png', {
e2e	UNKNOWN STEP	          |                              ^
e2e	UNKNOWN STEP	      603 |       maxDiffPixelRatio: 0.02,
e2e	UNKNOWN STEP	      604 |       timeout: 10_000,
e2e	UNKNOWN STEP	      605 |     });
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/explorer-visual.spec.js:602:30
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1278331Z ##[error]  35) tests/e2e/explorer-visual.spec.js:618:3 › Explorer toolbar visual regression › toolbar_renders_with_lens_toggle_and_stats 
e2e	UNKNOWN STEP	    Error: expect(locator).toHaveScreenshot(expected) failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('.treemap-toolbar')
e2e	UNKNOWN STEP	      Expected an image 919px by 74px, received 679px by 89px. 29933 pixels (ratio 0.37 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      Snapshot: toolbar-default.png
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toHaveScreenshot(toolbar-default.png)" with timeout 10000ms
e2e	UNKNOWN STEP	        - verifying given screenshot expectation
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-toolbar')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-toolbar svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - Expected an image 919px by 74px, received 679px by 89px. 29933 pixels (ratio 0.37 of all image pixels) are different.
e2e	UNKNOWN STEP	      - waiting 100ms before taking screenshot
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-toolbar')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-toolbar svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - captured a stable screenshot
e2e	UNKNOWN STEP	      - Expected an image 919px by 74px, received 679px by 89px. 29933 pixels (ratio 0.37 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      622 |     await expect(toolbar).toBeVisible({ timeout: 5_000 });
e2e	UNKNOWN STEP	      623 |
e2e	UNKNOWN STEP	    > 624 |     await expect(toolbar).toHaveScreenshot('toolbar-default.png', {
e2e	UNKNOWN STEP	          |                           ^
e2e	UNKNOWN STEP	      625 |       maxDiffPixelRatio: 0.02,
e2e	UNKNOWN STEP	      626 |       timeout: 10_000,
e2e	UNKNOWN STEP	      627 |     });
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/explorer-visual.spec.js:624:27
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1297213Z ##[error]  36) tests/e2e/explorer-visual.spec.js:630:3 › Explorer toolbar visual regression › evaluative_lens_toggle_changes_toolbar 
e2e	UNKNOWN STEP	    Error: expect(locator).toHaveScreenshot(expected) failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('.treemap-toolbar')
e2e	UNKNOWN STEP	      Expected an image 919px by 74px, received 679px by 113px. 46039 pixels (ratio 0.45 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      Snapshot: toolbar-evaluative.png
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toHaveScreenshot(toolbar-evaluative.png)" with timeout 10000ms
e2e	UNKNOWN STEP	        - verifying given screenshot expectation
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-toolbar')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-toolbar svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - Expected an image 919px by 74px, received 679px by 113px. 46039 pixels (ratio 0.45 of all image pixels) are different.
e2e	UNKNOWN STEP	      - waiting 100ms before taking screenshot
e2e	UNKNOWN STEP	      - waiting for locator('.treemap-toolbar')
e2e	UNKNOWN STEP	        - locator resolved to <div class="treemap-toolbar svelte-17jdq88">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - captured a stable screenshot
e2e	UNKNOWN STEP	      - Expected an image 919px by 74px, received 679px by 113px. 46039 pixels (ratio 0.45 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      637 |
e2e	UNKNOWN STEP	      638 |     const toolbar = page.locator('.treemap-toolbar');
e2e	UNKNOWN STEP	    > 639 |     await expect(toolbar).toHaveScreenshot('toolbar-evaluative.png', {
e2e	UNKNOWN STEP	          |                           ^
e2e	UNKNOWN STEP	      640 |       maxDiffPixelRatio: 0.02,
e2e	UNKNOWN STEP	      641 |       timeout: 10_000,
e2e	UNKNOWN STEP	      642 |     });
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/explorer-visual.spec.js:639:27
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1318390Z ##[error]  37) tests/e2e/explorer-visual.spec.js:655:3 › Explorer full page visual regression › full_explorer_page_default_state 
e2e	UNKNOWN STEP	    Error: expect(locator).toHaveScreenshot(expected) failed
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Locator: locator('[role="tabpanel"]')
e2e	UNKNOWN STEP	      Expected an image 1279px by 561px, received 1039px by 561px. 145219 pixels (ratio 0.21 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      Snapshot: explorer-full-page.png
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	    Call log:
e2e	UNKNOWN STEP	      - Expect "toHaveScreenshot(explorer-full-page.png)" with timeout 10000ms
e2e	UNKNOWN STEP	        - verifying given screenshot expectation
e2e	UNKNOWN STEP	      - waiting for locator('[role="tabpanel"]')
e2e	UNKNOWN STEP	        - locator resolved to <div tabindex="0" role="tabpanel" id="tabpanel-architecture" class="tab-content svelte-iz3u1" aria-labelledby="tab-architecture">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - Expected an image 1279px by 561px, received 1039px by 561px. 145219 pixels (ratio 0.21 of all image pixels) are different.
e2e	UNKNOWN STEP	      - waiting 100ms before taking screenshot
e2e	UNKNOWN STEP	      - waiting for locator('[role="tabpanel"]')
e2e	UNKNOWN STEP	        - locator resolved to <div tabindex="0" role="tabpanel" id="tabpanel-architecture" class="tab-content svelte-iz3u1" aria-labelledby="tab-architecture">…</div>
e2e	UNKNOWN STEP	      - taking element screenshot
e2e	UNKNOWN STEP	        - disabled all CSS animations
e2e	UNKNOWN STEP	      - waiting for fonts to load...
e2e	UNKNOWN STEP	      - fonts loaded
e2e	UNKNOWN STEP	      - attempting scroll into view action
e2e	UNKNOWN STEP	        - waiting for element to be stable
e2e	UNKNOWN STEP	      - captured a stable screenshot
e2e	UNKNOWN STEP	      - Expected an image 1279px by 561px, received 1039px by 561px. 145219 pixels (ratio 0.21 of all image pixels) are different.
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	
e2e	UNKNOWN STEP	      657 |
e2e	UNKNOWN STEP	      658 |     const tabPanel = page.locator('[role="tabpanel"]');
e2e	UNKNOWN STEP	    > 659 |     await expect(tabPanel).toHaveScreenshot('explorer-full-page.png', {
e2e	UNKNOWN STEP	          |                            ^
e2e	UNKNOWN STEP	      660 |       maxDiffPixelRatio: 0.02,
e2e	UNKNOWN STEP	      661 |       timeout: 10_000,
e2e	UNKNOWN STEP	      662 |     });
e2e	UNKNOWN STEP	        at /home/runner/work/gyre/gyre/web/tests/e2e/explorer-visual.spec.js:659:28
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1347743Z ##[notice]  37 failed
e2e	UNKNOWN STEP	    tests/e2e/app.spec.js:50:3 › App shell › no_sidebar_present ────────────────────────────────────
e2e	UNKNOWN STEP	    tests/e2e/app.spec.js:155:3 › Workspace home › decisions_section_renders_or_shows_empty_state ──
e2e	UNKNOWN STEP	    tests/e2e/app.spec.js:171:3 › Workspace home › repos_section_renders_or_shows_empty_state ──────
e2e	UNKNOWN STEP	    tests/e2e/app.spec.js:187:3 › Workspace home › briefing_section_renders ────────────────────────
e2e	UNKNOWN STEP	    tests/e2e/app.spec.js:195:3 › Workspace home › specs_section_renders ───────────────────────────
e2e	UNKNOWN STEP	    tests/e2e/app.spec.js:203:3 › Workspace home › agent_rules_section_renders ─────────────────────
e2e	UNKNOWN STEP	    tests/e2e/app.spec.js:254:3 › Repo mode › horizontal_tabs_render_in_repo_mode ──────────────────
e2e	UNKNOWN STEP	    tests/e2e/app.spec.js:270:3 › Repo mode › specs_tab_is_default_landing_tab ─────────────────────
e2e	UNKNOWN STEP	    tests/e2e/app.spec.js:278:3 › Repo mode › back_arrow_visible_in_topbar_in_repo_mode ────────────
e2e	UNKNOWN STEP	    tests/e2e/app.spec.js:286:3 › Repo mode › repo_breadcrumb_shows_workspace_slash_repo ───────────
e2e	UNKNOWN STEP	    tests/e2e/app.spec.js:294:3 › Repo mode › back_arrow_returns_to_workspace_home ─────────────────
e2e	UNKNOWN STEP	    tests/e2e/app.spec.js:308:3 › Repo mode › repo_header_renders_with_repo_name ───────────────────
e2e	UNKNOWN STEP	    tests/e2e/app.spec.js:319:3 › Repo mode › architecture_tab_renders_and_is_active ───────────────
e2e	UNKNOWN STEP	    tests/e2e/app.spec.js:331:3 › Repo mode › decisions_tab_renders_and_is_active ──────────────────
e2e	UNKNOWN STEP	    tests/e2e/app.spec.js:339:3 › Repo mode › specs_tab_renders_spec_list_or_empty_state ───────────
e2e	UNKNOWN STEP	    tests/e2e/app.spec.js:362:3 › URL routing › workspace_home_url_loads ───────────────────────────
e2e	UNKNOWN STEP	    tests/e2e/app.spec.js:370:3 › URL routing › repo_url_loads_with_tab_bar ────────────────────────
e2e	UNKNOWN STEP	    tests/e2e/app.spec.js:379:3 › URL routing › repo_architecture_url_activates_architecture_tab ───
e2e	UNKNOWN STEP	    tests/e2e/app.spec.js:394:3 › URL routing › browser_back_forward_navigation ────────────────────
e2e	UNKNOWN STEP	    tests/e2e/app.spec.js:506:3 › Keyboard shortcuts › g_1_navigates_to_specs_tab_in_repo_mode ─────
e2e	UNKNOWN STEP	    tests/e2e/app.spec.js:520:3 › Keyboard shortcuts › g_2_navigates_to_architecture_tab_in_repo_mode 
e2e	UNKNOWN STEP	    tests/e2e/app.spec.js:623:3 › Accessibility › repo_tab_bar_has_tablist_role ────────────────────
e2e	UNKNOWN STEP	    tests/e2e/explorer-visual.spec.js:308:3 › Semantic zoom visual regression › zoom_level_0_packages_overview 
e2e	UNKNOWN STEP	    tests/e2e/explorer-visual.spec.js:331:3 › Semantic zoom visual regression › zoom_level_1_modules 
e2e	UNKNOWN STEP	    tests/e2e/explorer-visual.spec.js:355:3 › Semantic zoom visual regression › zoom_level_2_types_and_functions 
e2e	UNKNOWN STEP	    tests/e2e/explorer-visual.spec.js:388:3 › View query rendering visual regression › view_query_with_groups_callouts_narrative 
e2e	UNKNOWN STEP	    tests/e2e/explorer-visual.spec.js:403:3 › View query rendering visual regression › view_query_container_with_annotation_bar 
e2e	UNKNOWN STEP	    tests/e2e/explorer-visual.spec.js:427:3 › Filter presets visual regression › filter_all_shows_complete_graph 
e2e	UNKNOWN STEP	    tests/e2e/explorer-visual.spec.js:438:3 › Filter presets visual regression › filter_endpoints_shows_only_endpoints 
e2e	UNKNOWN STEP	    tests/e2e/explorer-visual.spec.js:456:3 › Filter presets visual regression › filter_types_shows_only_type_nodes 
e2e	UNKNOWN STEP	    tests/e2e/explorer-visual.spec.js:487:3 › Filter presets visual regression › filter_calls_shows_call_graph 
e2e	UNKNOWN STEP	    tests/e2e/explorer-visual.spec.js:511:3 › Filter presets visual regression › filter_dependencies_shows_dependency_edges 
e2e	UNKNOWN STEP	    tests/e2e/explorer-visual.spec.js:545:3 › Blast radius visual regression › blast_radius_tiered_coloring_on_node_click 
e2e	UNKNOWN STEP	    tests/e2e/explorer-visual.spec.js:575:3 › Blast radius visual regression › blast_radius_dimmed_unmatched_nodes 
e2e	UNKNOWN STEP	    tests/e2e/explorer-visual.spec.js:618:3 › Explorer toolbar visual regression › toolbar_renders_with_lens_toggle_and_stats 
e2e	UNKNOWN STEP	    tests/e2e/explorer-visual.spec.js:630:3 › Explorer toolbar visual regression › evaluative_lens_toggle_changes_toolbar 
e2e	UNKNOWN STEP	    tests/e2e/explorer-visual.spec.js:655:3 › Explorer full page visual regression › full_explorer_page_default_state 
e2e	UNKNOWN STEP	  27 passed (2.9m)
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1351324Z ##[error]Process completed with exit code 1.
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1420751Z ##[group]Run actions/upload-artifact@v4
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1420841Z with:
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1420952Z   name: playwright-screenshots
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1421209Z   path: web/test-results/
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1421302Z   if-no-files-found: warn
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1421388Z   compression-level: 6
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1421473Z   overwrite: false
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1421569Z   include-hidden-files: false
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1421643Z env:
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1421759Z   FORCE_JAVASCRIPT_ACTIONS_TO_NODE24: true
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1421852Z   CARGO_HOME: /home/runner/.cargo
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1421939Z   CARGO_INCREMENTAL: 0
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1422026Z   CARGO_TERM_COLOR: always
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1422112Z   CACHE_ON_FAILURE: false
e2e	UNKNOWN STEP	2026-10-08T22:06:49.1422192Z ##[endgroup]
e2e	UNKNOWN STEP	2026-10-08T22:06:49.3085747Z (node:11479) [DEP0040] DeprecationWarning: The `punycode` module is deprecated. Please use a userland alternative instead.
e2e	UNKNOWN STEP	2026-10-08T22:06:49.3087631Z (Use `node --trace-deprecation ...` to show where the warning was created)
e2e	UNKNOWN STEP	2026-10-08T22:06:49.3517424Z With the provided path, there will be 82 files uploaded
e2e	UNKNOWN STEP	2026-10-08T22:06:49.3518657Z Artifact name is valid!
e2e	UNKNOWN STEP	2026-10-08T22:06:49.3519353Z Root directory input is valid!
e2e	UNKNOWN STEP	2026-10-08T22:06:49.7641126Z Beginning upload of artifact content to blob storage
e2e	UNKNOWN STEP	2026-10-08T22:06:49.8684583Z (node:11479) [DEP0169] DeprecationWarning: `url.parse()` behavior is not standardized and prone to errors that have security implications. Use the WHATWG URL API instead. CVEs are not issued for `url.parse()` vulnerabilities.
e2e	UNKNOWN STEP	2026-10-08T22:06:50.4953895Z Uploaded bytes 486419
e2e	UNKNOWN STEP	2026-10-08T22:06:50.5816488Z Finished uploading artifact content to blob storage!
e2e	UNKNOWN STEP	2026-10-08T22:06:50.5818507Z SHA256 digest of uploaded artifact zip is 2cb229a1d04148d49cb9102889430e5834c7d8da0c3d7ac20725faae19645af6
e2e	UNKNOWN STEP	2026-10-08T22:06:50.5819741Z Finalizing artifact upload
e2e	UNKNOWN STEP	2026-10-08T22:06:50.8425627Z Artifact playwright-screenshots.zip successfully finalized. Artifact ID 11581649769
e2e	UNKNOWN STEP	2026-10-08T22:06:50.8428211Z Artifact playwright-screenshots has been successfully uploaded! Final size is 486419 bytes. Artifact ID is 11581649769
e2e	UNKNOWN STEP	2026-10-08T22:06:50.8434563Z Artifact download URL: https://github.com/jsell-rh/gyre/actions/runs/37849768152/artifacts/11581649769
e2e	UNKNOWN STEP	2026-10-08T22:06:50.8659900Z Post job cleanup.
e2e	UNKNOWN STEP	2026-10-08T22:06:50.9505442Z [command]/usr/bin/git version
e2e	UNKNOWN STEP	2026-10-08T22:06:50.9553714Z git version 2.55.0
e2e	UNKNOWN STEP	2026-10-08T22:06:50.9594610Z Temporarily overriding HOME='/home/runner/work/_temp/de456ed3-ad73-4ed5-b838-3e7f07c6cfe1' before making global git config changes
e2e	UNKNOWN STEP	2026-10-08T22:06:50.9596374Z Adding repository directory to the temporary git global config as a safe directory
e2e	UNKNOWN STEP	2026-10-08T22:06:50.9601545Z [command]/usr/bin/git config --global --add safe.directory /home/runner/work/gyre/gyre
e2e	UNKNOWN STEP	2026-10-08T22:06:50.9641318Z [command]/usr/bin/git config --local --name-only --get-regexp core\.sshCommand
e2e	UNKNOWN STEP	2026-10-08T22:06:50.9695080Z [command]/usr/bin/git submodule foreach --recursive sh -c "git config --local --name-only --get-regexp 'core\.sshCommand' && git config --local --unset-all 'core.sshCommand' || :"
e2e	UNKNOWN STEP	2026-10-08T22:06:50.9927819Z [command]/usr/bin/git config --local --name-only --get-regexp http\.https\:\/\/github\.com\/\.extraheader
e2e	UNKNOWN STEP	2026-10-08T22:06:50.9965235Z http.https://github.com/.extraheader
e2e	UNKNOWN STEP	2026-10-08T22:06:50.9977193Z [command]/usr/bin/git config --local --unset-all http.https://github.com/.extraheader
e2e	UNKNOWN STEP	2026-10-08T22:06:51.0018291Z [command]/usr/bin/git submodule foreach --recursive sh -c "git config --local --name-only --get-regexp 'http\.https\:\/\/github\.com\/\.extraheader' && git config --local --unset-all 'http.https://github.com/.extraheader' || :"
e2e	UNKNOWN STEP	2026-10-08T22:06:51.0281885Z [command]/usr/bin/git config --local --name-only --get-regexp ^includeIf\.gitdir:
e2e	UNKNOWN STEP	2026-10-08T22:06:51.0332231Z [command]/usr/bin/git submodule foreach --recursive git config --local --show-origin --name-only --get-regexp remote.origin.url
e2e	UNKNOWN STEP	2026-10-08T22:06:51.0723885Z Cleaning up orphan processes
e2e	UNKNOWN STEP	2026-10-08T22:06:51.1073990Z ##[warning]Node.js 20 is deprecated. The following actions target Node.js 20 but are being forced to run on Node.js 24: actions/checkout@v4, actions/setup-node@v4, actions/upload-artifact@v4. For more information see: https://github.blog/changelog/2025-09-19-deprecation-of-node-20-on-github-actions-runners/

```
