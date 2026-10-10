const assert = require('node:assert/strict');
const fs = require('node:fs');
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
    assert.equal(await page.getByLabel('Amount (mL)').getAttribute('inputmode'), 'numeric');
    const save = await page.getByRole('button', { name: 'Save' }).boundingBox();
    assert.ok(save.y + save.height <= 844 - 80, `Save sits under a mobile browser toolbar at ${save.y}`);
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
    await page.getByRole('navigation', { name: 'Primary navigation' }).last()
      .getByRole('button', { name: 'Today' }).click();
    await page.getByRole('button', { name: /^Sleep/ }).click();
    await page.getByText('Sleep · running').waitFor();
    assert.equal(await page.getByRole('button', { name: /^Sleep/ }).count(), 0, 'running sleep offered a second start');
    await page.clock.fastForward(65 * 60_000);
    assert.equal(await page.locator('.sleep-running strong').textContent(), '65:00');
    await page.reload();
    await page.getByText(/^Sleeping since/).waitFor();
    await page.getByRole('button', { name: 'Stop sleep' }).click();
    await page.getByText('Sleep · 1 hr 5 min').waitFor();
    assert.equal(await page.locator('.sleep-running').count(), 0, 'stopped sleep still shown as running');
    assert.equal(await page.locator('.summary strong').textContent(), '2 feeds · 1 diaper');
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
    await page.locator('.side-nav').getByRole('button', { name: 'Family' }).click();
    let download;
    try {
      [download] = await Promise.all([
        page.waitForEvent('download', { timeout: 8000 }),
        page.getByRole('button', { name: 'Export readable backup' }).click({ timeout: 8000 }),
      ]);
    } catch (cause) {
      throw new Error(`Backup export: ${cause.message}; screen: ${await page.locator('main').innerText()}`);
    }
    const backup = fs.readFileSync(await download.path());
    assert.match(backup.toString('utf8'), /"kind":"babytrack-backup"/);
    const freshContext = await browser.newContext({ viewport: { width: 390, height: 844 } });
    const freshPage = await freshContext.newPage();
    await freshPage.goto(origin);
    await freshPage.getByLabel('Choose a backup file').setInputFiles({
      name: 'family.jsonl', mimeType: 'application/x-ndjson', buffer: backup,
    });
    await freshPage.getByText('Bottle · 90 mL').waitFor();
    await freshPage.reload();
    await freshPage.getByText('Breastfeed · left 1:05 · right 0:15').waitFor();
    await freshPage.getByText('Sleep · 1 hr 5 min').waitFor();
    await freshContext.close();
    await page.getByLabel('Restore file into a new Family').setInputFiles({
      name: 'family.jsonl', mimeType: 'application/x-ndjson', buffer: backup,
    });
    await page.waitForFunction((oldFamily) =>
      localStorage.getItem('babytrack-family') !== oldFamily, firstFamily);
    await page.reload();
    await page.getByText('Bottle · 90 mL').waitFor();
    await page.getByText('Breastfeed · left 1:05 · right 0:15').waitFor();
    const restoredFamily = await page.evaluate(() => localStorage.getItem('babytrack-family'));
    await page.locator('.side-nav').getByRole('button', { name: 'Family' }).click();
    await page.getByLabel('Restore file into a new Family').setInputFiles({
      name: 'broken.jsonl', mimeType: 'application/x-ndjson', buffer: backup.subarray(0, backup.length - 1),
    });
    await page.getByRole('alert').waitFor();
    assert.equal(await page.evaluate(() => localStorage.getItem('babytrack-family')),
      restoredFamily, 'corrupt file changed the active Family');
    assert.deepEqual(failures, []);
    await context.close();
    console.log('Responsive web local tracking, sleep timer, backup restore, reload, and Family isolation passed');
  } finally { await browser.close(); }
})().catch((error) => { console.error(error); process.exitCode = 1; });
