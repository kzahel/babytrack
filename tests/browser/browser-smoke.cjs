const assert = require('node:assert/strict');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');
const { chromium } = require('playwright');

const generatedDir = process.argv[2];
if (!generatedDir) throw new Error('pass wasm-bindgen web output directory');
const fixtures = JSON.parse(fs.readFileSync(path.join(__dirname, '../vectors/negative-batch-v1.json')));
const minor = fixtures.cases.find((entry) => entry.id === 'CROSSMINORBYTE01').input;
const input = {
  familyHex: fixtures.base.family_id_hex,
  otherFamilyHex: `ff${fixtures.base.family_id_hex.slice(2)}`,
  relayHex: '03396219237f75a64f12aeb7f39723abf400b160c364980a765dac24aeba2464',
  keyHex: fixtures.base.epoch_key_hex,
  seedHex: fixtures.base.recipient_sign_seed_hex,
  headerHex: minor.header_cbor_hex,
  operationHex: minor.operation_hex,
  envelopeHex: minor.envelope_cbor_hex,
  recordHex: '0183f9d0000070008000000000000011',
};

const server = http.createServer((request, response) => {
  if (request.url === '/') {
    response.writeHead(200, { 'Content-Type': 'text/html' });
    response.end('<!doctype html><title>babytrack wasm storage smoke</title>');
    return;
  }
  const name = request.url.slice(1);
  if (!['babytrack_core_wasm.js', 'babytrack_core_wasm_bg.wasm'].includes(name)) {
    response.writeHead(404).end();
    return;
  }
  response.writeHead(200, {
    'Content-Type': name.endsWith('.wasm') ? 'application/wasm' : 'text/javascript',
  });
  fs.createReadStream(path.join(generatedDir, name)).pipe(response);
});

async function run() {
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  let browser;
  try {
    browser = await chromium.launch({ headless: true });
    const context = await browser.newContext();
    const page = await context.newPage();
    page.on('pageerror', (error) => { throw error; });
    const url = `http://127.0.0.1:${server.address().port}/`;
    await page.goto(url);

    const saved = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const bytes = (hex) => Uint8Array.from(hex.match(/../g), (pair) => parseInt(pair, 16));
      const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
      const openDb = () => new Promise((resolve, reject) => {
        const request = indexedDB.open('babytrack-m0-smoke', 1);
        request.onupgradeneeded = () => {
          const db = request.result;
          db.createObjectStore('keys', { keyPath: 'family' });
          db.createObjectStore('batches', { keyPath: ['family', 'cursor'] });
          db.createObjectStore('projection', { keyPath: ['family', 'record', 'field'] });
        };
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
      });
      const completed = (tx) => new Promise((resolve, reject) => {
        tx.oncomplete = resolve;
        tx.onerror = () => reject(tx.error);
        tx.onabort = () => reject(tx.error);
      });
      const sealed = wasm.seal_one(bytes(data.headerHex), bytes(data.operationHex),
        bytes(data.keyHex), bytes(data.seedHex));
      if (hex(sealed) !== data.envelopeHex) throw new Error('browser wasm seal bytes differ');
      const family = new wasm.WasmFamily(bytes(data.familyHex));
      const signer = wasm.ed25519_public_key(bytes(data.seedHex));
      if (!family.apply_envelope(sealed, bytes(data.relayHex), bytes(data.keyHex), signer, 1n)) {
        throw new Error('browser wasm replay failed');
      }
      const field1 = family.field_cbor(bytes(data.recordHex), 1n);
      const field500 = family.field_cbor(bytes(data.recordHex), 500n);
      const db = await openDb();
      const tx = db.transaction(['keys', 'batches', 'projection'], 'readwrite');
      tx.objectStore('keys').put({ family: data.familyHex, key: bytes(data.keyHex) });
      tx.objectStore('batches').put({ family: data.familyHex, cursor: 1, envelope: sealed });
      tx.objectStore('projection').put({ family: data.familyHex, record: data.recordHex, field: 1, bytes: field1 });
      tx.objectStore('projection').put({ family: data.familyHex, record: data.recordHex, field: 500, bytes: field500 });
      await completed(tx);
      db.close();
      return [hex(field1), hex(field500)];
    }, input);
    assert.deepEqual(saved, ['6442616279', 'f4']);

    await page.reload();
    const reloaded = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const bytes = (hex) => Uint8Array.from(hex.match(/../g), (pair) => parseInt(pair, 16));
      const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
      const openDb = () => new Promise((resolve, reject) => {
        const request = indexedDB.open('babytrack-m0-smoke', 1);
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
      });
      const requestValue = (request) => new Promise((resolve, reject) => {
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
      });
      const db = await openDb();
      const read = db.transaction(['keys', 'batches', 'projection']);
      const key = await requestValue(read.objectStore('keys').get(data.familyHex));
      const row = await requestValue(read.objectStore('batches').get([data.familyHex, 1]));
      const retained = await requestValue(read.objectStore('projection').get([data.familyHex, data.recordHex, 500]));
      const otherKey = await requestValue(read.objectStore('keys').get(data.otherFamilyHex));
      const otherRow = await requestValue(read.objectStore('batches').get([data.otherFamilyHex, 1]));
      const family = new wasm.WasmFamily(bytes(data.familyHex));
      const signer = wasm.ed25519_public_key(bytes(data.seedHex));
      if (!family.apply_envelope(row.envelope, bytes(data.relayHex), key.key, signer, 1n)) {
        throw new Error('browser reload replay failed');
      }
      const projected = hex(family.field_cbor(bytes(data.recordHex), 500n));
      const rollback = db.transaction(['batches', 'projection'], 'readwrite');
      rollback.objectStore('batches').put({ family: data.familyHex, cursor: 2, envelope: row.envelope });
      rollback.objectStore('projection').put({ family: data.familyHex, record: data.recordHex, field: 500, bytes: bytes('f5') });
      const aborted = new Promise((resolve) => { rollback.onabort = resolve; });
      rollback.abort();
      await aborted;
      db.close();
      return { projected, retained: hex(retained.bytes), otherKey: otherKey === undefined, otherRow: otherRow === undefined };
    }, input);
    assert.deepEqual(reloaded, { projected: 'f4', retained: 'f4', otherKey: true, otherRow: true });

    await page.reload();
    const rolledBack = await page.evaluate(async (data) => {
      const db = await new Promise((resolve, reject) => {
        const request = indexedDB.open('babytrack-m0-smoke', 1);
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
      });
      const tx = db.transaction(['batches', 'projection']);
      const get = (request) => new Promise((resolve, reject) => {
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
      });
      const absent = await get(tx.objectStore('batches').get([data.familyHex, 2]));
      const field = await get(tx.objectStore('projection').get([data.familyHex, data.recordHex, 500]));
      const otherKey = new Uint8Array(32).fill(1);
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const bytes = (hex) => Uint8Array.from(hex.match(/../g), (pair) => parseInt(pair, 16));
      const signer = wasm.ed25519_public_key(bytes(data.seedHex));
      const family = new wasm.WasmFamily(bytes(data.familyHex));
      let wrongKeyRejected = false;
      try {
        family.apply_envelope(bytes(data.envelopeHex), bytes(data.relayHex), otherKey, signer, 1n);
      } catch { wrongKeyRejected = true; }
      db.close();
      return { absent: absent === undefined, field: Array.from(field.bytes), wrongKeyRejected };
    }, input);
    assert.deepEqual(rolledBack, { absent: true, field: [0xf4], wrongKeyRejected: true });
    await context.close();
    console.log('browser wasm + IndexedDB reload, rollback, and Family key isolation: OK');
  } finally {
    if (browser) await browser.close();
    await new Promise((resolve) => server.close(resolve));
  }
}

run().catch((error) => { console.error(error); process.exitCode = 1; });
