const assert = require('node:assert/strict');
const fs = require('node:fs');
const { chromium } = require('playwright');

const origin = process.argv[2];
if (!origin) throw new Error('pass the preview origin');

// History opens on today's Day view; entry checks use All days so a run
// near local midnight still finds entries saved a little earlier.
async function historyTab(page) {
  await page.locator('.bottom-nav:visible, .side-nav:visible').getByRole('button', { name: 'History' }).first().click();
  await page.getByRole('button', { name: 'All days' }).click();
}
const today = (page) => page.getByRole('navigation', { name: 'Primary navigation' }).last()
  .getByRole('button', { name: 'Today' }).click();

// Each Android capture type through the chooser, checked by its saved summary.
async function captureEveryKind(page) {
  const open = async (kind) => {
    await today(page);
    await page.locator('.actions-grid').getByRole('button', { name: /^More/ }).click();
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
  const earlier = new Date(await page.evaluate(() => Date.now() - 5 * 60_000));
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
  await page.locator('.child-row strong').getByText('Sammy').first().waitFor();
  await page.getByRole('button', { name: 'Edit Sammy' }).click();
  assert.equal(await page.getByRole('button', { name: 'Female' }).getAttribute('aria-pressed'), 'true');
  assert.equal(await page.getByLabel('Birthday').inputValue(), '2026-06-01');
  await page.getByRole('button', { name: '← Cancel' }).click();
  await today(page);
}

// Android's per-type corrections, made from History rows.
async function correctEveryKind(page) {
  const correct = async (entry, action, change, result) => {
    await historyTab(page);
    await page.locator('.entry-main').filter({ hasText: entry }).first().click();
    await page.getByRole('button', { name: action }).click();
    await change();
    await page.getByRole('button', { name: 'Save changes' }).click();
    await page.getByRole('button', { name: 'Save changes' }).waitFor({ state: 'detached', timeout: 8000 }).catch(async (cause) => {
      throw new Error(`${action}: ${cause.message}\n${await page.locator('main').innerText()}`);
    });
    await historyTab(page);
    await page.getByText(result, { exact: true }).first().waitFor();
  };
  await correct('Bottle · 4.5 US fl oz', 'Edit bottle', async () => {
    await page.getByRole('button', { name: 'mL', exact: true }).click();
    await page.getByLabel('Bottle amount (mL)').fill('120');
    await page.getByRole('button', { name: 'Formula' }).click();
  }, 'Bottle · 120 mL · Formula');
  await correct('Pump · 90 mL total', 'Edit pumping amounts', async () => {
    await page.getByLabel('Total (mL), instead of sides').fill('');
    await page.getByLabel('Left (mL)').fill('40');
    await page.getByLabel('Right (mL)').fill('35');
  }, 'Pump · L 40 mL · R 35 mL · 12 min');
  await correct('Solids · pear, oats', 'Edit solids', async () => {
    await page.getByLabel('Foods (one per line)').fill('rice');
  }, 'Solids · rice · half a bowl');
  await correct('Sleep · 45 min', 'Edit sleep duration', async () => {
    await page.getByLabel('Minutes slept').fill('30');
  }, 'Sleep · 30 min');
  await correct('Sleep · 30 min', 'Edit sleep place', async () => {
    await page.getByRole('button', { name: 'Car' }).click();
  }, 'Place: Car');
  await correct('Sleep · 30 min', 'Move session', async () => {
    const start = await page.getByLabel('New start').inputValue();
    const earlier = new Date(new Date(start).getTime() - 3_600_000);
    await page.getByLabel('New start').fill(new Date(earlier.getTime() - earlier.getTimezoneOffset() * 60_000)
      .toISOString().slice(0, 16));
  }, 'Sleep · 30 min');
  await correct('Growth · 5.25 kg', 'Edit growth', async () => {
    await page.getByRole('button', { name: 'cm', exact: true }).last().click();
    await page.getByLabel('Head circumference', { exact: true }).fill('38');
  }, 'Growth · 5.25 kg · 23 in · head 38 cm');
  await correct('Temperature · 98.6 °F', 'Edit temperature', async () => {
    await page.getByRole('button', { name: '°C' }).click();
    await page.getByLabel('Temperature (°C)').fill('37,5');
  }, 'Temperature · 37.5 °C');
  await correct('Medication · Vitamin D', 'Edit medication entry', async () => {
    await page.getByLabel('Medication name').fill('Iron');
  }, 'Medication · Iron · 1 drop');
  await correct('Medication · Iron', 'Add note to entry', async () => {
    await page.getByLabel('Note').fill('After lunch');
  }, 'Note: After lunch');
  await correct('Note · Rolled over', 'Edit time', async () => {
    const at = new Date(new Date(await page.getByLabel('When').inputValue()).getTime() - 3_600_000);
    await page.getByLabel('When').fill(new Date(at.getTime() - at.getTimezoneOffset() * 60_000).toISOString().slice(0, 16));
  }, 'Note · Rolled over');
  await correct('Breast · Right 5 min', 'Edit breast feed', async () => {
    await page.getByRole('button', { name: 'Keep start time' }).click();
    await page.getByLabel('Feeding time (minutes:seconds)').last().fill('9:00');
    await page.getByRole('button', { name: 'Save changes' }).click();
    await page.getByText('The edited feeding cannot end in the future.').first().waitFor();
    await page.getByRole('button', { name: 'Keep finish time' }).click();
  }, 'Breast · Right 5 min → Left 9 min');
  await historyTab(page);
  await page.locator('.entry-main').filter({ hasText: 'Medication · Iron' }).click();
  page.once('dialog', (dialog) => dialog.accept());
  await page.getByRole('button', { name: 'Delete entry' }).click();
  await page.getByText('Entry deleted').first().waitFor();
  assert.equal(await page.getByText('Medication · Iron').count(), 0, 'deleted entry still listed');
  await page.getByRole('button', { name: 'Undo' }).click();
  await page.getByText('Medication · Iron · 1 drop').first().waitFor();
  await today(page);
}

// Day view: core totals for the chosen day, filters, and the week strip.
async function checkDayView(page) {
  await page.locator('.bottom-nav').getByRole('button', { name: 'History' }).click();
  await page.locator('.day-totals-row').waitFor();
  assert.match(await page.locator('.day-totals-row').innerText(), /feeds/);
  await page.getByRole('button', { name: 'Sleep', exact: true }).click();
  const rows = await page.locator('.entry-main strong').allInnerTexts();
  assert.ok(rows.length && rows.every((text) => text.startsWith('Sleep')), `sleep filter showed ${rows}`);
  await page.locator('.week-strip button').first().click();
  await page.getByText('No entries in this view.').waitFor();
  await page.locator('.week-strip button').last().click();
  await page.getByRole('button', { name: 'All', exact: true }).click();
  await page.getByRole('button', { name: 'All days' }).click();
  await page.getByRole('heading', { name: 'Today' }).waitFor();
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
    await page.getByRole('button', { name: 'Add your child' }).click();
    await page.getByLabel('Name or nickname').fill('Sam');
    await page.getByRole('button', { name: /^Birthday/ }).click();
    await page.getByLabel('Birth date (optional)').fill('2026-06-01');
    await page.getByRole('button', { name: 'Start tracking' }).click();
    await page.getByRole('heading', { name: 'Sam' }).waitFor();
    await page.locator('.actions-grid').getByRole('button', { name: /^Wet/ }).click();
    await page.getByText('Diaper · Wet').first().waitFor();
    await page.locator('.actions-grid').getByRole('button', { name: /^Bottle/ }).click();
    assert.equal(await page.getByLabel('Bottle amount (mL)').getAttribute('inputmode'), 'numeric');
    const save = await page.getByRole('button', { name: 'Save bottle' }).boundingBox();
    assert.ok(save.y + save.height <= 844 - 80, `Save sits under a mobile browser toolbar at ${save.y}`);
    await page.getByLabel('Bottle amount (mL)').fill('90');
    await page.getByRole('button', { name: 'Save bottle' }).click();
    await page.getByText('Bottle · 90 mL').first().waitFor();
    const timerStart = new Date(Date.now() + 1000);
    await page.clock.install({ time: timerStart });
    await page.clock.pauseAt(new Date(timerStart.getTime() + 1000));
    await page.locator('.actions-grid').getByRole('button', { name: /^Breast/ }).click();
    await page.getByRole('button', { name: /Left breast/ }).click();
    await page.clock.fastForward(30_000);
    await page.reload();
    await page.locator('.now-card').click();
    await page.getByRole('button', { name: /Right breast/ }).click();
    await page.clock.fastForward(15_000);
    await page.getByRole('button', { name: /Right breast/ }).click();
    await page.clock.fastForward(20_000);
    assert.equal(await page.locator('.breast-totals strong').last().textContent(), '0:15', 'pause counted as feeding');
    await page.getByRole('button', { name: /Left breast/ }).click();
    await page.clock.fastForward(45_000);
    await page.getByRole('button', { name: /Left breast/ }).click();
    await page.getByRole('button', { name: 'Save feeding' }).click();
    await page.getByText('Breast · Left 0:30 → Right 0:15 → Left 0:45').first().waitFor();
    assert.equal(await page.locator('.now-card').count(), 0, 'saved feeding retained its draft');
    await historyTab(page);
    await page.locator('.entry-main').filter({ hasText: 'Breast · Left 0:30' }).click();
    await page.getByRole('button', { name: 'Edit breast feed' }).click();
    await page.getByLabel('Feeding time (minutes:seconds)').first().fill('0:20');
    await page.getByRole('button', { name: 'Save changes' }).click();
    await page.getByText('Breast · Left 0:20 → Right 0:15 → Left 0:45').first().waitFor();
    await page.getByRole('navigation', { name: 'Primary navigation' }).last()
      .getByRole('button', { name: 'Today' }).click();
    await page.locator('.actions-grid').getByRole('button', { name: /^Sleep/ }).click();
    await page.getByText('Sleep · running').first().waitFor();
    assert.match(await page.locator('.actions-grid').getByRole('button', { name: /^Sleep/ }).innerText(), /Open timer/,
      'running sleep offered a second start');
    await page.clock.fastForward(65 * 60_000);
    assert.equal(await page.locator('.now-card strong').textContent(), '1:05:00');
    await page.reload();
    await page.locator('.now-card').filter({ hasText: 'Sleeping since' }).click();
    await page.getByRole('button', { name: 'Stop sleep' }).click();
    await page.getByText('Sleep · 1 h 5 min').first().waitFor();
    assert.equal(await page.locator('.now-card').count(), 0, 'stopped sleep still shown as running');
    await page.getByText('Napped 1 h 5 min').first().waitFor();
    assert.match(await page.locator('.day-totals-row').innerText(),
      /2 feeds\s+90 mL bottle\s+1 h 5 min\s+sleep\s+1 diaper\s+1 wet · 0 dirty/);
    await captureEveryKind(page);
    await correctEveryKind(page);
    await checkDayView(page);
    const firstFamily = await page.evaluate(() => localStorage.getItem('babytrack-family'));
    await page.reload();
    await historyTab(page);
    await page.getByText('Diaper · Wet').first().waitFor();
    await page.getByText('Bottle · 90 mL').first().waitFor();
    await page.getByText('Breast · Left 0:20 → Right 0:15 → Left 0:45').first().waitFor();
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
    await page.getByText('Bottle · 90 mL').first().waitFor();
    await page.locator('.side-nav').getByRole('button', { name: 'Family' }).click();
    let download;
    try {
      [download] = await Promise.all([
        page.waitForEvent('download', { timeout: 8000 }),
        page.getByRole('button', { name: 'Save backup' }).click({ timeout: 8000 }),
      ]);
    } catch (cause) {
      throw new Error(`Backup export: ${cause.message}; screen: ${await page.locator('main').innerText()}`);
    }
    const backup = fs.readFileSync(await download.path());
    assert.match(backup.toString('utf8'), /"kind":"babytrack-backup"/);
    await page.getByText(/^Last completed file save:/).waitFor();
    const [csv] = await Promise.all([page.waitForEvent('download'),
      page.getByRole('button', { name: 'Export analysis CSV' }).click()]);
    assert.equal(csv.suggestedFilename(), 'babytrack-analysis.csv');
    const rows = fs.readFileSync(await csv.path(), 'utf8');
    assert.ok(rows.startsWith('family_id,child_id,child_name,') && rows.includes(',"feed.bottle",'), rows.slice(0, 200));
    await page.getByRole('button', { name: 'Protect with password' }).click();
    await page.getByLabel('Backup password').fill('correct horse');
    const [locked] = await Promise.all([page.waitForEvent('download', { timeout: 60_000 }),
      page.getByRole('button', { name: 'Save backup' }).click()]);
    assert.equal(locked.suggestedFilename(), 'babytrack-backup.btbk');
    const protectedBackup = fs.readFileSync(await locked.path());
    assert.equal(protectedBackup.subarray(0, 5).toString(), 'BTBK1');
    assert.equal(protectedBackup.includes(Buffer.from('babytrack-backup')), false, 'protected file is readable');
    const freshContext = await browser.newContext({ viewport: { width: 390, height: 844 } });
    const freshPage = await freshContext.newPage();
    await freshPage.goto(origin);
    await freshPage.getByRole('button', { name: 'Restore a backup' }).click();
    await freshPage.getByLabel('Restore file into a new Family').setInputFiles({
      name: 'family.btbk', mimeType: 'application/octet-stream', buffer: protectedBackup,
    });
    await freshPage.getByLabel('Password for protected backup').fill('wrong horse');
    await freshPage.getByRole('button', { name: 'Check protected backup' }).click();
    await freshPage.getByText('Wrong password or damaged backup file.').waitFor({ timeout: 60_000 });
    assert.ok(!await freshPage.evaluate(() => localStorage.getItem('babytrack-family')), 'wrong password made a Family');
    await freshPage.getByLabel('Password for protected backup').fill('correct horse');
    await freshPage.getByRole('button', { name: 'Check protected backup' }).click();
    await freshPage.getByText(/^File saved at .* records$/).waitFor({ timeout: 60_000 });
    await freshPage.getByRole('button', { name: 'Restore as new Family' }).click();
    await historyTab(freshPage);
    await freshPage.getByText('Bottle · 90 mL').first().waitFor();
    await freshPage.reload();
    await historyTab(freshPage);
    await freshPage.getByText('Breast · Left 0:20 → Right 0:15 → Left 0:45').first().waitFor();
    await freshPage.getByText('Sleep · 1 h 5 min').first().waitFor();
    await freshContext.close();
    await page.getByLabel('Restore file into a new Family').setInputFiles({
      name: 'family.jsonl', mimeType: 'application/x-ndjson', buffer: backup,
    });
    await page.getByRole('button', { name: 'Restore as new Family' }).click();
    await page.waitForFunction((oldFamily) =>
      localStorage.getItem('babytrack-family') !== oldFamily, firstFamily);
    await page.reload();
    await historyTab(page);
    await page.getByText('Bottle · 90 mL').first().waitFor();
    await page.getByText('Breast · Left 0:20 → Right 0:15 → Left 0:45').first().waitFor();
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
