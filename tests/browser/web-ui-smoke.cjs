const assert = require('node:assert/strict');
const fs = require('node:fs');
const { chromium } = require('playwright');

const origin = process.argv[2];
if (!origin) throw new Error('pass the preview origin');

const historyTab = (page) => page.locator('.bottom-nav:visible, .side-nav:visible')
  .getByRole('button', { name: 'History' }).first().click();
const today = (page) => page.getByRole('navigation', { name: 'Primary navigation' }).last()
  .getByRole('button', { name: 'Today' }).click();

// Each Android capture type through the chooser, checked by its saved summary.
async function captureEveryKind(page) {
  const open = async (kind) => {
    await today(page);
    await page.getByRole('button', { name: /^＋|More/ }).click();
    await page.getByRole('button', { name: new RegExp(`^${kind}$`) }).click();
  };
  const saved = async (button, text) => {
    await page.getByRole('button', { name: button }).click();
    await page.getByRole('heading', { name: 'Sam' }).waitFor();
    await page.locator('.bottom-nav').getByRole('button', { name: 'History' }).click();
    await page.getByText(text, { exact: true }).first().waitFor();
  };
  await open('Bottle');
  await page.getByRole('button', { name: 'US fl oz' }).click();
  assert.equal(await page.getByLabel('Bottle amount (US fl oz)').getAttribute('inputmode'), 'decimal');
  await page.getByRole('button', { name: 'More', exact: true }).click();
  await page.getByRole('button', { name: 'More', exact: true }).click();
  assert.equal(await page.getByLabel('Bottle amount (US fl oz)').inputValue(), '1');
  await page.getByLabel('Bottle amount (US fl oz)').fill('4,5');
  await page.getByRole('button', { name: 'Breast milk' }).click();
  await saved('Save bottle', 'Bottle · 4.5 US fl oz · Breast milk');
  await open('Bottle');
  await page.getByRole('button', { name: /Same as last: 4.5 US fl oz/ }).click();
  assert.equal(await page.getByLabel('Bottle amount (US fl oz)').inputValue(), '4.5');
  await page.getByRole('button', { name: '← Cancel' }).click();
  await open('Pumping');
  await page.getByRole('button', { name: 'Start pumping timer' }).click();
  await page.clock.fastForward(12 * 60_000);
  await page.getByRole('button', { name: 'Stop timer' }).click();
  assert.equal(await page.getByLabel('Minutes pumping').inputValue(), '12');
  await page.getByLabel('Total (mL), instead of sides').fill('90');
  await page.getByLabel('Left (mL)').fill('40');
  assert.ok(await page.getByRole('button', { name: 'Save pumping' }).isDisabled(), 'pump accepted sides and total');
  await page.getByLabel('Left (mL)').fill('');
  await saved('Save pumping', 'Pump · 90 mL total · 12 min');
  await open('Solids');
  await page.getByLabel('Foods (one per line)').fill(' pear \n\n oats ');
  await page.getByLabel('Amount eaten (optional)').fill('half a bowl');
  await saved('Save solids', 'Solids · pear, oats · half a bowl');
  await open('Sleep');
  await page.getByLabel('Minutes slept').fill('45');
  await page.getByRole('button', { name: 'Pram' }).click();
  await saved('Save sleep', 'Sleep · 45 min');
  await open('Growth');
  await page.getByLabel('Weight', { exact: true }).fill('5,25');
  await page.getByRole('button', { name: 'in', exact: true }).first().click();
  await page.getByLabel('Length', { exact: true }).fill('23');
  await saved('Save growth', 'Growth · 5.25 kg · 23 in');
  await open('Temperature');
  await page.getByRole('button', { name: '°F' }).click();
  await page.getByLabel('Temperature (°F)').fill('98.6');
  await saved('Save temperature', 'Temperature · 98.6 °F');
  await open('Medication');
  await page.getByLabel('Medication name').fill('Vitamin D');
  await page.getByLabel('Dose amount').fill('1');
  await page.getByLabel('Dose unit').fill('drop');
  await saved('Save medication', 'Medication · Vitamin D · 1 drop');
  await open('Note');
  await page.getByRole('button', { name: 'Change time' }).click();
  const earlier = new Date(Date.now() - 2 * 3_600_000);
  const local = new Date(earlier.getTime() - earlier.getTimezoneOffset() * 60_000).toISOString().slice(0, 16);
  await page.getByLabel('When', { exact: true }).last().fill(local);
  await page.getByLabel('What happened?').fill('Rolled over');
  await saved('Save note', 'Note · Rolled over');
  await open('Breastfeed');
  await page.getByRole('button', { name: 'Enter minutes' }).click();
  await page.getByRole('button', { name: 'Right', exact: true }).click();
  await page.getByLabel('Minutes on selected side').fill('5');
  await page.getByRole('button', { name: 'Add segment' }).click();
  await page.getByLabel('Minutes on selected side').fill('7');
  await saved('Save breast feed', 'Breast · Right 5 min → Left 7 min');
  await page.locator('.bottom-nav').getByRole('button', { name: 'Family' }).click();
  await page.getByRole('button', { name: 'Edit Sam' }).click();
  await page.getByLabel('Child name').fill('Sammy');
  await page.getByRole('button', { name: 'Female' }).click();
  await page.getByRole('button', { name: 'Save changes' }).click();
  await page.locator('.child-row strong').getByText('Sammy').waitFor();
  await page.getByRole('button', { name: 'Edit Sammy' }).click();
  assert.equal(await page.getByRole('button', { name: 'Female' }).getAttribute('aria-pressed'), 'true');
  assert.equal(await page.getByLabel('Birthday').inputValue(), '2026-06-01');
  await page.getByRole('button', { name: '← Cancel' }).click();
  await today(page);
}

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
    await page.getByRole('button', { name: 'Add a child' }).click();
    await page.getByRole('button', { name: /^Diaper/ }).click();
    await page.getByRole('button', { name: 'Wet' }).click();
    await page.getByRole('button', { name: 'Save diaper' }).click();
    await page.getByText('Diaper · Wet').waitFor();
    await page.getByRole('button', { name: /^Bottle/ }).click();
    assert.equal(await page.getByLabel('Bottle amount (mL)').getAttribute('inputmode'), 'numeric');
    const save = await page.getByRole('button', { name: 'Save bottle' }).boundingBox();
    assert.ok(save.y + save.height <= 844 - 80, `Save sits under a mobile browser toolbar at ${save.y}`);
    await page.getByLabel('Bottle amount (mL)').fill('90');
    await page.getByRole('button', { name: 'Save bottle' }).click();
    await page.getByText('Bottle · 90 mL').waitFor();
    const timerStart = new Date(Date.now() + 1000);
    await page.clock.install({ time: timerStart });
    await page.clock.pauseAt(new Date(timerStart.getTime() + 1000));
    await page.getByRole('button', { name: /^Breastfeed/ }).click();
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
    await page.getByText('Breast · Left 0:30 → Right 0:15 → Left 0:45').waitFor();
    assert.equal(await page.locator('.timer-resume').count(), 0, 'saved feeding retained its draft');
    await page.getByRole('button', { name: 'Edit' }).click();
    await page.getByLabel('Feeding time (minutes:seconds)').first().fill('0:20');
    await page.getByRole('button', { name: 'Save changes' }).click();
    await page.getByText('Breast · Left 0:20 → Right 0:15 → Left 0:45').waitFor();
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
    await page.getByText('Sleep · 1 h 5 min').waitFor();
    assert.equal(await page.locator('.sleep-running').count(), 0, 'stopped sleep still shown as running');
    assert.equal(await page.locator('.summary strong').textContent(), '2 feeds · 1 diaper');
    await captureEveryKind(page);
    const firstFamily = await page.evaluate(() => localStorage.getItem('babytrack-family'));
    await page.reload();
    await historyTab(page);
    await page.getByText('Diaper · Wet').waitFor();
    await page.getByText('Bottle · 90 mL').waitFor();
    await page.getByText('Breast · Left 0:20 → Right 0:15 → Left 0:45').waitFor();
    await page.getByRole('navigation', { name: 'Primary navigation' }).last()
      .getByRole('button', { name: 'Family' }).click();
    await page.getByRole('button', { name: 'Create a Family' }).click();
    await page.getByLabel('Child name').fill('Kai');
    await page.getByRole('button', { name: 'Add a child' }).click();
    await page.getByRole('navigation', { name: 'Primary navigation' }).last()
      .getByRole('button', { name: 'Today' }).click();
    await page.getByRole('heading', { name: 'Kai' }).waitFor();
    assert.equal(await page.getByText('Diaper · Wet').count(), 0, 'new Family leaked old history');
    assert.equal(await page.getByText('Breast · Left 0:20 → Right 0:15 → Left 0:45').count(), 0, 'new Family leaked feeding history');
    await page.reload();
    await page.getByRole('heading', { name: 'Kai' }).waitFor();
    await page.setViewportSize({ width: 1200, height: 800 });
    await page.locator('.side-nav')
      .getByRole('button', { name: 'Family' }).click();
    await page.getByLabel('Switch Family').selectOption(firstFamily);
    await historyTab(page);
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
    await historyTab(freshPage);
    await freshPage.getByText('Bottle · 90 mL').waitFor();
    await freshPage.reload();
    await historyTab(freshPage);
    await freshPage.getByText('Breast · Left 0:20 → Right 0:15 → Left 0:45').waitFor();
    await freshPage.getByText('Sleep · 1 h 5 min').waitFor();
    await freshContext.close();
    await page.getByLabel('Restore file into a new Family').setInputFiles({
      name: 'family.jsonl', mimeType: 'application/x-ndjson', buffer: backup,
    });
    await page.waitForFunction((oldFamily) =>
      localStorage.getItem('babytrack-family') !== oldFamily, firstFamily);
    await page.reload();
    await historyTab(page);
    await page.getByText('Bottle · 90 mL').waitFor();
    await page.getByText('Breast · Left 0:20 → Right 0:15 → Left 0:45').waitFor();
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
