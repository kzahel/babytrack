const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { pathToFileURL } = require('node:url');
const { chromium } = require('playwright');

const root = path.resolve(process.argv[2] || 'apps/android/app/build/outputs/screen-gallery');
async function screenshot(page, name) {
  if (!process.env.BABYTRACK_GALLERY_SCREENSHOTS) return;
  const directory = path.resolve(process.env.BABYTRACK_GALLERY_SCREENSHOTS);
  fs.mkdirSync(directory, { recursive: true });
  await page.screenshot({ path: path.join(directory, name) });
}
async function run() {
  const browser = await chromium.launch({ headless: true });
  try {
    for (const viewport of [{ width: 1440, height: 1000 }, { width: 390, height: 844 }]) {
      const context = await browser.newContext({ viewport, offline: true });
      try {
        const page = await context.newPage();
        const errors = [];
        page.on('pageerror', (error) => errors.push(error.message));
        await page.goto(pathToFileURL(path.join(root, 'index.html')).href);
        const data = await page.locator('#gallery-data').evaluate((node) => JSON.parse(node.textContent));
        const count = data.cases.length;
        assert.equal(await page.locator('.case-card').count(), count);
        assert.equal(await page.locator('.preview').count(), count);
        assert.deepEqual(await page.locator('.preview').evaluateAll((nodes) => [...new Set(nodes.map((node) => node.dataset.variant))]), ['dark']);
        assert.equal(await page.locator('.destination').count(), data.groups.length);
        if (data.cases.some((item) => item.id === 'today-typical')) {
          assert.equal(await page.locator('.destination[data-group=today] .case-card').first().getAttribute('data-case'), 'today-typical');
        }
        if (data.cases.some((item) => item.id === 'capture-bottle-empty')) {
          assert.equal(await page.locator('[data-case=capture-bottle-empty] h3').innerText(), 'Bottle · empty draft');
        }
        for (const group of data.groups) {
          await page.locator(`#groups [data-group="${group.id}"]`).click();
          assert.equal(await page.locator('.case-card').count(), data.cases.filter((item) => item.group === group.id).length);
        }
        await page.locator('#reset').click();
        for (const state of data.states) {
          await page.locator('#state').selectOption(state.id);
          const matching = data.cases.filter((item) => item.state === state.id);
          assert.equal(await page.locator('.case-card').count(), matching.length);
          for (const group of data.groups) {
            assert.equal(await page.locator(`#groups [data-group="${group.id}"] .count`).innerText(), String(matching.filter((item) => item.group === group.id).length));
          }
        }
        await page.locator('#reset').click();
        // Every combination keeps only requested variants; comparisons stay complete.
        for (const theme of ['dark', 'light', 'both']) {
          for (const text of ['normal', 'large', 'both']) {
            await page.locator('#theme').selectOption(theme);
            await page.locator('#text').selectOption(text);
            const expected = ['dark', 'light', 'dark-large', 'light-large'].filter((variant) =>
              (theme === 'both' || variant.startsWith(theme)) &&
              (text === 'both' || variant.endsWith('large') === (text === 'large')));
            assert.equal(await page.locator('.preview').count(), count * expected.length);
            assert.deepEqual(await page.locator('.preview').evaluateAll((nodes) => [...new Set(nodes.map((node) => node.dataset.variant))]), expected);
            assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
            if (theme === 'both' && text === 'both') await screenshot(page, `gallery-comparison-${viewport.width}.png`);
          }
        }
        await page.locator('#reset').click();
        if (data.groups.some((group) => group.id === 'family')) {
          await page.locator('#groups [data-group=family]').click();
          assert.equal(await page.locator('.case-card').count(), data.cases.filter((item) => item.group === 'family').length);
          assert.equal(await page.locator('#groups [data-group=family]').getAttribute('aria-pressed'), 'true');
          assert.equal(await page.locator('#groups [data-group=family]').evaluate((node) => node === document.activeElement), true);
          await page.locator('#state').selectOption('pending');
          assert.equal(await page.locator('.case-card').count(), data.cases.filter((item) => item.group === 'family' && item.state === 'pending').length);
          await page.locator('#reset').click();
          await page.locator('#search').fill('family pending');
          assert.equal(await page.locator('.case-card').count(), data.cases.filter((item) => item.group === 'family' && item.state === 'pending').length);
        }
        await page.locator('#search').fill('no-such-fixture-98234');
        assert.equal(await page.locator('.case-card').count(), 0);
        assert.equal(await page.locator('#empty').isVisible(), true);
        await page.locator('#empty-reset').click();
        assert.equal(await page.locator('.case-card').count(), count);
        // Focused galleries can contain only single-screen fixtures.
        const long = data.cases.find((item) => item.variants.dark.pages.length > 1);
        const inspected = long || data.cases[0];
        const opener = page.locator(`[data-case="${inspected.id}"] .preview-button`).first();
        await opener.click();
        assert.equal(await page.locator('#viewer').isVisible(), true);
        assert.equal(await page.locator('#viewer-title').innerText(), inspected.label);
        assert.equal(await page.locator('#previous-page').isDisabled(), true);
        const firstSource = await page.locator('#viewer-image').getAttribute('src');
        if (long) {
          await page.locator('#next-page').click();
          assert.notEqual(await page.locator('#viewer-image').getAttribute('src'), firstSource);
          assert((await page.locator('#page-status').innerText()).startsWith('2 /'));
          await page.keyboard.press('ArrowLeft');
          assert.equal(await page.locator('#viewer-image').getAttribute('src'), firstSource);
        } else {
          assert.equal(await page.locator('#next-page').isDisabled(), true);
        }
        await page.locator('#viewer-variant').selectOption('light-large');
        assert.equal(await page.locator('#viewer-image').getAttribute('src'), inspected.variants['light-large'].pages[0].file);
        const original = await page.locator('#original').getAttribute('href');
        assert(fs.existsSync(path.join(root, original)));
        await page.waitForFunction(() => {
          const image = document.querySelector('#viewer-image');
          return image.complete && image.naturalWidth > 0;
        });
        await screenshot(page, `gallery-viewer-${viewport.width}.png`);
        await page.keyboard.press('Escape');
        assert.equal(await page.locator('#viewer').isVisible(), false);
        assert.equal(await opener.evaluate((node) => node === document.activeElement), true);
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
        await page.evaluate(() => { document.activeElement.blur(); window.scrollTo(0, 0); });
        await screenshot(page, `gallery-${viewport.width}.png`);
        assert.deepEqual(errors, []);
        console.log(`Gallery ${viewport.width}px: organization, filters, comparisons, viewer, keyboard, responsive layout OK`);
      } finally { await context.close(); }
    }
  } finally { await browser.close(); }
}
run().catch((error) => { console.error(error); process.exitCode = 1; });
