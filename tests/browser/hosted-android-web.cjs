// Cross-client preview smoke. The caller supplies a disposable HTTPS relay
// origin and its public verification key; this script never reads its seed.
const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const { chromium } = require('playwright');

const origin = process.env.BABYTRACK_PREVIEW_ORIGIN;
const relayPublicKey = process.env.BABYTRACK_RELAY_PUBLIC_KEY;
const serial = process.env.BABYTRACK_ANDROID_SERIAL || 'emulator-5554';
assert.match(origin || '', /^https:\/\/[^/]+$/);
assert.match(relayPublicKey || '', /^[0-9a-f]{64}$/);

function adb(...args) {
  const result = spawnSync('adb', ['-s', serial, ...args], {
    encoding: 'utf8', timeout: 90000,
  });
  assert.equal(result.status, 0, `adb ${args[0]} failed: ${result.stderr || result.stdout}`);
  return result.stdout.trim();
}

function step(name, extras = {}) {
  const args = ['shell', 'am', 'instrument', '-w', '-e', 'class',
    `org.babytrack.app.TwoDeviceRelayTest#${name}`,
    '-e', 'relayOrigin', origin];
  for (const [key, value] of Object.entries(extras)) args.push('-e', key, value);
  args.push('org.babytrack.app.test/androidx.test.runner.AndroidJUnitRunner');
  const output = adb(...args);
  assert.match(output, /OK \(1 test\)/, `${name}: ${output}`);
  process.stdout.write(`${name}: OK\n`);
}

async function run() {
  step('managerCreate', { relayPublicKey });
  const fragment = adb('exec-out', 'run-as', 'org.babytrack.app',
    'cat', 'files/two-device-link.txt');
  assert.match(fragment, /^#bt-invite=v1\.[A-Za-z0-9_-]+$/);
  const marker = Date.now().toString(36);
  const androidNote = `AndroidHostedSmoke${marker}`;
  const browserNote = `BrowserHostedSmoke${marker}`;

  const browser = await chromium.launch({ headless: true });
  try {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 } });
    const page = await context.newPage();
    const pageErrors = [];
    page.on('pageerror', (error) => pageErrors.push(error.message));
    await page.goto(`${origin}/${fragment}`);
    await page.getByText('Invitation ready to join').waitFor();
    await page.getByRole('button', { name: 'Join this Family' }).click();
    await page.getByText('Claim sent · waiting for the manager device').waitFor();
    assert.equal(new URL(page.url()).hash, '', 'invitation secret stayed in URL');
    process.stdout.write('browser claim: OK\n');

    step('managerRespond');
    await page.getByRole('button', { name: 'Resume joining' }).click();
    await page.getByText('Proof sent · waiting for the Family key grant').waitFor();
    process.stdout.write('browser proof: OK\n');

    step('managerAdmit');
    await page.getByRole('button', { name: 'Resume joining' }).click();
    await page.getByRole('heading', { name: 'Shared child' }).waitFor();
    await page.locator('.bottom-nav').getByRole('button', { name: 'Family' }).click();
    await page.locator('.child-row strong').getByText('Shared child').waitFor();
    await page.getByRole('heading', { name: 'Shared Family' }).first().waitFor();
    process.stdout.write('browser verified history: OK\n');

    step('managerWriteHostedNote', { noteMarker: androidNote });
    await page.getByRole('button', { name: 'Sync now' }).click();
    await page.locator('.bottom-nav').getByRole('button', { name: 'Today' }).click();
    await page.getByText(androidNote).waitFor();
    process.stdout.write('Android to browser note: OK\n');

    await page.locator('.quick-grid button').filter({ hasText: 'Note' }).click();
    await page.getByLabel('Note').fill(browserNote);
    await page.getByRole('button', { name: 'Save' }).click();
    await page.getByText(browserNote).waitFor();
    await page.locator('.bottom-nav').getByRole('button', { name: 'Family' }).click();
    await page.getByText('Up to date').waitFor({ timeout: 30000 });
    step('managerReadsHostedNote', { noteMarker: browserNote });
    process.stdout.write('browser to Android note: OK\n');

    await page.reload();
    await page.locator('.bottom-nav').getByRole('button', { name: 'Today' }).click();
    await page.getByText(browserNote).waitFor();
    assert.deepEqual(pageErrors, []);
    process.stdout.write('browser reload and page errors: OK\n');
    await context.close();
  } finally {
    await browser.close();
  }
}

run().catch((error) => { console.error(error); process.exitCode = 1; });
