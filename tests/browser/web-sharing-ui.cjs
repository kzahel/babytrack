const assert = require('node:assert/strict');
const { spawn, spawnSync } = require('node:child_process');
const fs = require('node:fs');
const http = require('node:http');
const os = require('node:os');
const path = require('node:path');
const { chromium } = require('playwright');

const relayBin = path.resolve(process.argv[2] || '');
const holderBin = path.resolve(process.argv[3] || '');
if (!process.argv[2] || !process.argv[3]) throw new Error('pass relay and holder binaries');
const root = path.resolve(__dirname, '../..');

async function freePort() {
  const server = http.createServer();
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const port = server.address().port;
  await new Promise((resolve) => server.close(resolve));
  return port;
}

async function ready(origin) {
  for (let attempt = 0; attempt < 100; attempt++) {
    try { await fetch(origin); return; } catch { /* starting */ }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`Server did not start: ${origin}`);
}

async function deviceId(page) {
  return page.evaluate(async () => {
    const database = await new Promise((resolve, reject) => {
      const request = indexedDB.open('babytrack-preview-public-v1');
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
    const credential = await new Promise((resolve, reject) => {
      const request = database.transaction('credentials').objectStore('credentials').getAll();
      request.onsuccess = () => resolve(request.result[0]);
      request.onerror = () => reject(request.error);
    });
    database.close();
    return Array.from(credential.deviceId, (byte) => byte.toString(16).padStart(2, '0')).join('');
  });
}

async function run() {
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'babytrack-web-sharing-'));
  const manager = path.join(temporary, 'manager.db');
  const relayDb = path.join(temporary, 'relay.db');
  const seed = path.join(temporary, 'seed');
  const relayLog = path.join(temporary, 'relay.log');
  fs.writeFileSync(seed, Buffer.alloc(32, 0x6e));
  const webPort = await freePort();
  const relayPort = await freePort();
  const origin = `http://localhost:${webPort}`;
  const relayOrigin = `http://127.0.0.1:${relayPort}`;
  const webLog = fs.openSync(path.join(temporary, 'web.log'), 'w');
  const relayLogFd = fs.openSync(relayLog, 'w');
  const web = spawn('npm', ['run', 'dev', '--', '--host', 'localhost', '--port', String(webPort), '--strictPort'], {
    cwd: path.join(root, 'apps/web'),
    env: { ...process.env, BABYTRACK_RELAY_URL: relayOrigin },
    stdio: ['ignore', webLog, webLog],
  });
  let relay;
  let browser;
  let passed = false;
  const holder = (mode) => {
    const result = spawnSync(holderBin, [mode, manager, relayOrigin], { encoding: 'utf8' });
    assert.equal(result.status, 0, `Native holder ${mode}: ${result.stderr}`);
    return result.stdout.trim();
  };
  try {
    await ready(origin);
    const setup = spawnSync(holderBin, ['setup', manager, relayDb, origin], { encoding: 'utf8' });
    assert.equal(setup.status, 0, `Native holder setup: ${setup.stderr}`);
    const fragment = setup.stdout.trim();
    const startRelay = async () => {
      relay = spawn(relayBin, [relayDb, seed, `127.0.0.1:${relayPort}`],
        { stdio: ['ignore', relayLogFd, relayLogFd] });
      await ready(relayOrigin);
    };
    await startRelay();
    browser = await chromium.launch({ headless: true });
    const first = await browser.newContext({ viewport: { width: 390, height: 844 } });
    const page = await first.newPage();
    const failures = [];
    const controlPosts = [];
    page.on('pageerror', (error) => failures.push(error.message));
    page.on('request', (request) => {
      if (request.method() === 'POST' && request.url().includes('/control')) controlPosts.push(request.url());
    });
    await page.goto(origin + '/' + fragment);
    await page.getByText('Invitation ready to join').waitFor();
    assert.equal(controlPosts.length, 0, 'opening a link consumed an invitation');
    await page.getByText('Joining gives this browser access to the Family history').waitFor();
    await page.getByRole('button', { name: 'Join this Family' }).click();
    await page.getByText('Claim sent · waiting for the manager device').waitFor();
    assert.equal(new URL(page.url()).hash, '', 'invitation secret remained in URL');
    holder('challenge');
    await page.getByRole('button', { name: 'Resume joining' }).click();
    await page.getByText('Proof sent · waiting for the Family key grant').waitFor();
    holder('grant');
    await page.getByRole('button', { name: 'Resume joining' }).click();
    await page.getByText('Add a child to start tracking.').waitFor();
    const sharedFamily = await page.evaluate(() => localStorage.getItem('babytrack-family'));
    holder('write');
    await page.locator('.bottom-nav').getByRole('button', { name: 'Family' }).click();
    await page.getByRole('button', { name: 'Sync now' }).click();
    await page.locator('.child-row strong').getByText('DynamicHolderChild').waitFor();
    await page.locator('.bottom-nav').getByRole('button', { name: 'Today' }).click();
    await new Promise((resolve) => { relay.once('exit', resolve); relay.kill(); });
    await page.locator('.quick-grid button').filter({ hasText: 'Note' }).click();
    await page.getByLabel('Note').fill('WebOnlyPrivateMarker');
    await page.getByRole('button', { name: 'Save' }).click();
    await page.getByText('WebOnlyPrivateMarker').waitFor();
    await page.reload();
    await page.getByText('WebOnlyPrivateMarker').waitFor();
    await page.locator('.bottom-nav').getByRole('button', { name: 'Family' }).click();
    await page.getByText('Saved here · waiting to sync').waitFor();
    await startRelay();
    await page.getByRole('button', { name: 'Sync now' }).click();
    await page.getByText('Up to date').waitFor();
    assert.equal(holder('read_note'), 'browser note read');
    const marker = Buffer.from('WebOnlyPrivateMarker');
    assert.equal(fs.readFileSync(relayDb).includes(marker), false, 'plaintext reached relay DB');
    assert.equal(fs.readFileSync(relayLog).includes(marker), false, 'plaintext reached relay log');
    const canceled = spawnSync(holderBin, ['later_issue', manager, relayOrigin, origin],
      { encoding: 'utf8' });
    assert.equal(canceled.status, 0, `Native canceled issue: ${canceled.stderr}`);
    const cancel = spawnSync(holderBin, ['cancel', manager, relayOrigin, canceled.stdout.trim()],
      { encoding: 'utf8' });
    assert.equal(cancel.status, 0, `Native cancellation: ${cancel.stderr}`);
    const canceledContext = await browser.newContext();
    const canceledPage = await canceledContext.newPage();
    await canceledPage.goto(origin + '/' + canceled.stdout.trim());
    await canceledPage.getByText('Invitation ready to join').waitFor();
    await canceledPage.getByRole('button', { name: 'Join this Family' }).click();
    await canceledPage.getByText('This invitation was canceled. Ask a manager for a new link.').waitFor();
    await canceledPage.route('**/v1/**', (route) => route.abort());
    await canceledPage.reload();
    await canceledPage.getByText('This invitation was canceled. Ask a manager for a new link.').waitFor();
    await canceledPage.getByRole('button', { name: 'Dismiss invitation' }).click();
    await canceledPage.reload();
    assert.equal(await canceledPage.getByText('This invitation was canceled.', { exact: false }).count(), 0);
    await canceledContext.close();
    const later = spawnSync(holderBin, ['later_issue', manager, relayOrigin, origin],
      { encoding: 'utf8' });
    assert.equal(later.status, 0, `Native later issue: ${later.stderr}`);
    const second = await browser.newContext({ viewport: { width: 1024, height: 800 } });
    const secondPage = await second.newPage();
    secondPage.on('pageerror', (error) => failures.push(error.message));
    await secondPage.goto(origin + '/' + later.stdout.trim());
    await secondPage.getByText('Invitation ready to join').waitFor();
    let racedClaim = false;
    await secondPage.route('**/v1/families/*/control', (route) => {
      if (route.request().method() !== 'POST' || racedClaim) return route.continue();
      racedClaim = true;
      const advanced = spawnSync(holderBin, ['later_issue', manager, relayOrigin, origin],
        { encoding: 'utf8' });
      assert.equal(advanced.status, 0, `Native unrelated control: ${advanced.stderr}`);
      return route.continue();
    });
    await secondPage.getByRole('button', { name: 'Join this Family' }).click();
    await secondPage.getByText('Relay control commit failed: 409', { exact: false }).waitFor();
    assert.equal(racedClaim, true, 'claim did not race an unrelated control');
    await secondPage.getByRole('button', { name: 'Resume joining' }).click();
    await secondPage.getByText('Claim sent · waiting for the manager device').waitFor();
    await secondPage.unroute('**/v1/families/*/control');
    holder('later_challenge');
    let racedProof = false;
    await secondPage.route('**/v1/families/*/control', (route) => {
      if (route.request().method() !== 'POST' || racedProof) return route.continue();
      racedProof = true;
      const advanced = spawnSync(holderBin, ['later_issue', manager, relayOrigin, origin],
        { encoding: 'utf8' });
      assert.equal(advanced.status, 0, `Native control before proof: ${advanced.stderr}`);
      return route.continue();
    });
    await secondPage.getByRole('button', { name: 'Resume joining' }).click();
    await secondPage.getByText('Relay control commit failed: 409', { exact: false }).waitFor();
    assert.equal(racedProof, true, 'proof did not race an unrelated control');
    await secondPage.getByRole('button', { name: 'Resume joining' }).click();
    await secondPage.getByText('Proof sent · waiting for the Family key grant').waitFor();
    await secondPage.unroute('**/v1/families/*/control');
    holder('later_grant');
    await secondPage.getByRole('button', { name: 'Resume joining' }).click();
    await secondPage.locator('.side-nav').getByRole('button', { name: 'Family' }).click();
    await secondPage.locator('.child-row strong').getByText('DynamicHolderChild').waitFor();
    await secondPage.locator('.side-nav').getByRole('button', { name: 'Today' }).click();
    await secondPage.getByText('WebOnlyPrivateMarker').waitFor();
    const secondDevice = await deviceId(secondPage);
    await page.route('**/v1/**', (route) => route.abort());
    await page.locator('.bottom-nav').getByRole('button', { name: 'Today' }).click();
    await page.locator('.quick-grid button').filter({ hasText: 'Note' }).click();
    await page.getByLabel('Note').fill('RemovedPendingMarker');
    await page.getByRole('button', { name: 'Save' }).click();
    await page.getByText('RemovedPendingMarker').waitFor();
    const staleTab = await first.newPage();
    await staleTab.route('**/v1/**', (route) => route.abort());
    await staleTab.goto(origin);
    await staleTab.getByText('RemovedPendingMarker').waitFor();
    await staleTab.locator('.quick-grid button').filter({ hasText: 'Note' }).click();
    await staleTab.getByLabel('Note').fill('StaleTabUnsentMarker');
    const firstDevice = await deviceId(page);
    const removal = spawnSync(holderBin, ['remove', manager, relayOrigin, firstDevice],
      { encoding: 'utf8' });
    assert.equal(removal.status, 0, `Native removal: ${removal.stderr}`);
    await page.unroute('**/v1/**');
    await page.locator('.bottom-nav').getByRole('button', { name: 'Family' }).click();
    const syncFirst = page.getByRole('button', { name: 'Sync now' });
    if (await syncFirst.count()) await syncFirst.click({ timeout: 2000 }).catch(() => {});
    await page.getByText('Access ended · private copy saved on this browser').waitFor();
    await staleTab.getByRole('button', { name: 'Save' }).click();
    await staleTab.getByText('This action was saved in your independent local Family').waitFor();
    await staleTab.getByText('StaleTabUnsentMarker').waitFor();
    const redirectedFamily = await staleTab.evaluate(() => localStorage.getItem('babytrack-family'));
    await staleTab.close();
    await page.locator('.bottom-nav').getByRole('button', { name: 'Today' }).click();
    await page.getByRole('button', { name: 'Open private copy' }).click();
    await page.getByText('RemovedPendingMarker').waitFor();
    await page.getByText('StaleTabUnsentMarker').waitFor();
    assert.equal(await page.evaluate(() => localStorage.getItem('babytrack-family')), redirectedFamily);
    await page.reload();
    await page.getByText('RemovedPendingMarker').waitFor();
    await page.locator('.bottom-nav').getByRole('button', { name: 'Family' }).click();
    await page.getByLabel('Switch Family').selectOption(sharedFamily);
    await page.waitForFunction((id) => localStorage.getItem('babytrack-family') === id &&
      document.querySelector('.bottom-nav button.active')?.textContent.trim() === 'Today', sharedFamily);
    await page.locator('.bottom-nav').getByRole('button', { name: 'Family' }).click();
    const backupReady = page.waitForEvent('download', { timeout: 10000 }).catch(async (error) => {
      throw new Error(`${error.message}\nBackup screen: ${await page.locator('main').innerText()}`);
    });
    await page.getByRole('button', { name: 'Export readable backup' }).click();
    const backup = fs.readFileSync(await (await backupReady).path());
    await page.getByLabel('Restore file into a new Family').setInputFiles({
      name: 'shared.jsonl', mimeType: 'application/x-ndjson', buffer: backup,
    });
    await page.waitForFunction((oldFamily) =>
      localStorage.getItem('babytrack-family') !== oldFamily, sharedFamily);
    await page.getByText('RemovedPendingMarker').waitFor();
    const rotatedIssue = spawnSync(holderBin, ['later_issue', manager, relayOrigin, origin],
      { encoding: 'utf8' });
    assert.equal(rotatedIssue.status, 0, `Native rotated issue: ${rotatedIssue.stderr}`);
    const third = await browser.newContext({ viewport: { width: 390, height: 844 } });
    const thirdPage = await third.newPage();
    thirdPage.on('pageerror', (error) => failures.push(error.message));
    await thirdPage.goto(origin + '/' + rotatedIssue.stdout.trim());
    await thirdPage.getByText('Invitation ready to join').waitFor();
    let lostClaimResponse = false;
    await thirdPage.route('**/v1/families/*/control', async (route) => {
      if (route.request().method() !== 'POST' || lostClaimResponse) return route.continue();
      lostClaimResponse = true;
      const accepted = await route.fetch();
      assert.equal(accepted.status(), 200, 'claim was not committed before response loss');
      return route.abort();
    });
    await thirdPage.getByRole('button', { name: 'Join this Family' }).click();
    await thirdPage.getByText('Claim result is uncertain · retrying signed checks').waitFor();
    assert.equal(lostClaimResponse, true);
    await thirdPage.getByRole('button', { name: 'Resume joining' }).click();
    await thirdPage.getByText('Claim sent · waiting for the manager device').waitFor();
    await thirdPage.unroute('**/v1/families/*/control');
    holder('later_challenge');
    await thirdPage.getByRole('button', { name: 'Resume joining' }).click();
    await thirdPage.getByText('Proof sent · waiting for the Family key grant').waitFor();
    holder('later_grant');
    await thirdPage.getByRole('button', { name: 'Resume joining' }).click();
    await thirdPage.getByText('WebOnlyPrivateMarker').waitFor();
    await third.close();
    const secondRemoval = spawnSync(holderBin, ['remove', manager, relayOrigin, secondDevice],
      { encoding: 'utf8' });
    assert.equal(secondRemoval.status, 0, `Native second removal: ${secondRemoval.stderr}`);
    await secondPage.locator('.side-nav').getByRole('button', { name: 'Family' }).click();
    const syncSecond = secondPage.getByRole('button', { name: 'Sync now' });
    if (await syncSecond.count()) await syncSecond.click({ timeout: 2000 }).catch(() => {});
    await secondPage.getByText('Access ended · local archive available').waitFor();
    await secondPage.locator('.side-nav').getByRole('button', { name: 'Today' }).click();
    await secondPage.getByRole('button', { name: 'Make an independent copy' }).click();
    await secondPage.getByRole('button', { name: 'Open private copy' }).waitFor({ timeout: 5000 })
      .catch(async () => { throw new Error(`Manual copy screen: ${await secondPage.locator('main').innerText()}`); });
    await secondPage.getByRole('button', { name: 'Open private copy' }).click();
    await secondPage.getByText('WebOnlyPrivateMarker').waitFor();
    await second.close();
    assert.deepEqual(failures, []);
    passed = true;
    console.log('Web joins, rotated keys, removal copies, shared backup restore, offline edit, and reload passed');
  } finally {
    if (!passed) {
      console.error('Relay log:', fs.readFileSync(relayLog, 'utf8'));
      console.error('Web log:', fs.readFileSync(path.join(temporary, 'web.log'), 'utf8'));
    }
    await browser?.close();
    relay?.kill();
    web.kill();
    fs.closeSync(webLog);
    fs.closeSync(relayLogFd);
    fs.rmSync(temporary, { recursive: true, force: true });
  }
}

run().catch((error) => { console.error(error); process.exitCode = 1; });
