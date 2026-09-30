const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const scenarios = ['journal', 'authority', 'invitation', 'candidate-races', 'dynamic-enrollment', 'recipient-exchange', 'initial-exchange', 'rotation', 'rebase-recovery'];
const extra = process.argv.slice(7);
if (process.argv.includes('--list')) { console.log(scenarios.join('\n')); process.exit(0); }
let selected = scenarios;
if (extra.length) {
  if (extra.length !== 2 || extra[0] !== '--scenario') throw new Error('Use --scenario name[,name] or --list');
  selected = extra[1].split(',');
  for (const name of selected) if (!scenarios.includes(name)) throw new Error(`Unknown scenario: ${name}`);
}
const { chromium, server, startRelay, setRelay, disposeRelays, saveRelayLogs } = require('./browser-smoke/support.cjs');

async function run() {
  await new Promise((resolve) => server.listen(0, 'localhost', resolve));
  let browser;
  try {
    browser = await chromium.launch({ headless: true });
    for (const name of selected) {
      console.log(`Browser scenario ${name}: start`);
      const context = await browser.newContext();
      const pageErrors = [];
      context.on('page', (page) => page.on('pageerror', (error) => pageErrors.push(error)));
      await context.tracing.start({ screenshots: true, snapshots: true });
      try {
        const relay = await startRelay();
        setRelay(relay);
        const page = await context.newPage();
        const url = `http://localhost:${server.address().port}/`;
        await page.goto(url);
        await require(`./browser-smoke/${name}.cjs`)({ page, context, url, relay });
        assert.deepEqual(pageErrors, [], 'Unexpected browser errors');
        await context.tracing.stop();
        console.log(`Browser scenario ${name}: OK`);
      } catch (error) {
        const directory = path.resolve(process.env.BABYTRACK_BROWSER_DIAGNOSTICS_DIR || 'target/browser-diagnostics', `${name}-${Date.now()}`);
        try {
          fs.mkdirSync(directory, { recursive: true });
          fs.writeFileSync(path.join(directory, 'failure.txt'), `${error.stack || error}\n${pageErrors.map((item) => item.stack).join('\n')}`);
          saveRelayLogs(directory);
          await context.tracing.stop({ path: path.join(directory, 'trace.zip') });
          console.error(`Browser failure artifacts: ${directory}`);
        } catch (diagnosticError) { console.error('Could not save all browser diagnostics:', diagnosticError); }
        throw error;
      } finally {
        await context.close();
        await disposeRelays();
      }
    }
    console.log(`Browser wasm/IndexedDB/relay smoke: ${selected.length} scenario${selected.length === 1 ? '' : 's'} OK`);
  } finally {
    if (browser) await browser.close();
    await disposeRelays();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}
run().catch((error) => { console.error(error); process.exitCode = 1; });
