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
    const timerStart = new Date(Date.now() + 1000);
    await page.clock.install({ time: timerStart });
    await page.clock.pauseAt(new Date(timerStart.getTime() + 1000));
    await page.getByRole('button', { name: 'Breastfeed' }).click();
    await page.getByRole('button', { name: /Left breast/ }).click();
    await page.clock.fastForward(30_000);
    await page.reload();
    await page.locator('.timer-resume').click();
    await page.getByRole('button', { name: /Right breast/ }).click();
    await page.clock.fastForward(15_000);
    await page.getByRole('button', { name: /Right breast/ }).click();
    await page.clock.fastForward(20_000);
    assert.equal(await page.locator('.breast-totals strong').last().textContent(), '0:15', 'pause counted as feeding');
    await page.getByRole('button', { name: /Left breast/ }).click();
    await page.clock.fastForward(45_000);
    await page.getByRole('button', { name: /Left breast/ }).click();
    await page.getByRole('button', { name: 'Save feeding' }).click();
    await page.getByText('Breastfeed · left 1:15 · right 0:15').waitFor();
    assert.equal(await page.locator('.timer-resume').count(), 0, 'saved feeding retained its draft');
    await page.getByRole('button', { name: 'Edit' }).click();
    await page.getByLabel('Feeding time (minutes:seconds)').first().fill('0:20');
    await page.getByRole('button', { name: 'Save changes' }).click();
    await page.getByText('Breastfeed · left 1:05 · right 0:15').waitFor();
    const firstFamily = await page.evaluate(() => localStorage.getItem('babytrack-family'));
    await page.reload();
    await page.getByText('Diaper · Wet').waitFor();
    await page.getByText('Bottle · 90 mL').waitFor();
    await page.getByText('Breastfeed · left 1:05 · right 0:15').waitFor();
    await page.getByRole('navigation', { name: 'Primary navigation' }).last()
      .getByRole('button', { name: 'Family' }).click();
    await page.getByRole('button', { name: 'Create a Family' }).click();
    await page.getByLabel('Child name').fill('Kai');
    await page.getByRole('button', { name: 'Save' }).click();
    await page.getByRole('navigation', { name: 'Primary navigation' }).last()
      .getByRole('button', { name: 'Today' }).click();
    await page.getByRole('heading', { name: 'Kai' }).waitFor();
    assert.equal(await page.getByText('Diaper · Wet').count(), 0, 'new Family leaked old history');
    assert.equal(await page.getByText('Breastfeed · left 1:05 · right 0:15').count(), 0, 'new Family leaked feeding history');
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
