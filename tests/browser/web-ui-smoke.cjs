const assert = require('node:assert/strict');
const { chromium } = require('playwright');

const origin = process.argv[2];
if (!origin) throw new Error('pass the preview origin');

(async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 } });
    const page = await context.newPage();
    const failures = [];
    page.on('pageerror', (error) => failures.push(error.message));
    await page.goto(origin);
    await page.getByRole('button', { name: 'Create a Family' }).click();
    await page.getByLabel('Child name').fill('Sam');
    await page.getByLabel('Birthday').fill('2026-06-01');
    await page.getByRole('button', { name: 'Save' }).click();
    await page.getByRole('button', { name: 'Diaper' }).click();
    await page.getByRole('button', { name: 'Save' }).click();
    await page.getByText('Diaper · Wet').waitFor();
    await page.getByRole('button', { name: 'Bottle' }).click();
    await page.getByLabel('Amount (mL)').fill('90');
    await page.getByRole('button', { name: 'Save' }).click();
    await page.getByText('Bottle · 90 mL').waitFor();
    const firstFamily = await page.evaluate(() => localStorage.getItem('babytrack-family'));
    await page.reload();
    await page.getByText('Diaper · Wet').waitFor();
    await page.getByText('Bottle · 90 mL').waitFor();
    await page.getByRole('navigation', { name: 'Primary navigation' }).last()
      .getByRole('button', { name: 'Family' }).click();
    await page.getByRole('button', { name: 'Create a Family' }).click();
    await page.getByLabel('Child name').fill('Kai');
    await page.getByRole('button', { name: 'Save' }).click();
    await page.getByRole('navigation', { name: 'Primary navigation' }).last()
      .getByRole('button', { name: 'Today' }).click();
    await page.getByRole('heading', { name: 'Kai' }).waitFor();
    assert.equal(await page.getByText('Diaper · Wet').count(), 0, 'new Family leaked old history');
    await page.reload();
    await page.getByRole('heading', { name: 'Kai' }).waitFor();
    await page.setViewportSize({ width: 1200, height: 800 });
    await page.locator('.side-nav')
      .getByRole('button', { name: 'Family' }).click();
    await page.getByLabel('Switch Family').selectOption(firstFamily);
    await page.getByText('Bottle · 90 mL').waitFor();
    assert.deepEqual(failures, []);
    await context.close();
    console.log('Responsive web local tracking, reload, and Family isolation passed');
  } finally { await browser.close(); }
})().catch((error) => { console.error(error); process.exitCode = 1; });
