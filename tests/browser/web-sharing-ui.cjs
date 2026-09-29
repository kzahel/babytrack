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
    const page = await browser.newPage({ viewport: { width: 390, height: 844 } });
    const failures = [];
    page.on('pageerror', (error) => failures.push(error.message));
    await page.goto(origin + '/' + fragment);
    await page.getByText('Claim sent · waiting for the manager device').waitFor();
    assert.equal(new URL(page.url()).hash, '', 'invitation secret remained in URL');
    holder('challenge');
    await page.getByRole('button', { name: 'Resume joining' }).click();
    await page.getByText('Proof sent · waiting for the Family key grant').waitFor();
    holder('grant');
    await page.getByRole('button', { name: 'Resume joining' }).click();
    await page.getByText('Add a child to start tracking.').waitFor();
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
    await startRelay();
    await page.locator('.bottom-nav').getByRole('button', { name: 'Family' }).click();
    await page.getByRole('button', { name: 'Sync now' }).click();
    await page.getByText('Up to date').waitFor();
    assert.equal(holder('read_note'), 'browser note read');
    const marker = Buffer.from('WebOnlyPrivateMarker');
    assert.equal(fs.readFileSync(relayDb).includes(marker), false, 'plaintext reached relay DB');
    assert.equal(fs.readFileSync(relayLog).includes(marker), false, 'plaintext reached relay log');
    assert.deepEqual(failures, []);
    passed = true;
    console.log('Web join, delayed handoff, encrypted sync, offline edit, and reload passed');
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
