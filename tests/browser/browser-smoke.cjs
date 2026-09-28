const assert = require('node:assert/strict');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');
const { chromium } = require('playwright');

const generatedDir = process.argv[2];
if (!generatedDir) throw new Error('pass wasm-bindgen web output directory');
const fixtures = JSON.parse(fs.readFileSync(path.join(__dirname, '../vectors/negative-batch-v1.json')));
const full = JSON.parse(fs.readFileSync(path.join(__dirname, '../vectors/full-wire-v1.json')));
const chain = JSON.parse(fs.readFileSync(path.join(__dirname, '../vectors/contiguous-chain-v1.json')));
const minor = fixtures.cases.find((entry) => entry.id === 'CROSSMINORBYTE01').input;
const genesis = full.cases.find((entry) => entry.id === 'GENESIS01');
const acceptedBatch = full.cases.find((entry) => entry.id === 'BATCHBYTE01');
const cborBytes = (value) => {
  if (value.length < 256) return Buffer.concat([Buffer.from([0x58, value.length]), value]);
  const length = Buffer.alloc(3);
  length[0] = 0x59;
  length.writeUInt16BE(value.length, 1);
  return Buffer.concat([length, value]);
};
const oneEntryPage = (familyHex, kind, committed) => Buffer.concat([
  Buffer.from([0xa6, 1, 1, 2, 0x50]), Buffer.from(familyHex, 'hex'),
  Buffer.from([3, 1, 4, 0x81, 0x83, 2, kind]), cborBytes(committed),
  Buffer.from([5, 2, 6, 0xf4]),
]);
const issueBytes = Buffer.from(chain.transitions[1].committed_cbor_hex, 'hex');
const controlPage = oneEntryPage(chain.test_only_inputs.family_id_hex, 1, issueBytes);
const batchPage = oneEntryPage(genesis.inputs.family_id_hex, 2,
  Buffer.from(acceptedBatch.expect.envelope_cbor_hex, 'hex'));
const batchResult = Buffer.concat([
  Buffer.from([0xa2, 1, 1, 2]),
  cborBytes(Buffer.from(acceptedBatch.expect.accepted_receipt_cbor_hex, 'hex')),
]);
const input = {
  familyHex: fixtures.base.family_id_hex,
  otherFamilyHex: `ff${fixtures.base.family_id_hex.slice(2)}`,
  relayHex: '03396219237f75a64f12aeb7f39723abf400b160c364980a765dac24aeba2464',
  keyHex: fixtures.base.epoch_key_hex,
  seedHex: fixtures.base.recipient_sign_seed_hex,
  deviceHex: fixtures.base.recipient_id_hex,
  headerHex: minor.header_cbor_hex,
  operationHex: minor.operation_hex,
  envelopeHex: minor.envelope_cbor_hex,
  recordHex: '0183f9d0000070008000000000000011',
  publicGenesisHex: genesis.expect.control_object_hex,
  relayPublicHex: genesis.expect.relay_public_key_hex,
  acceptedEnvelopeHex: acceptedBatch.expect.envelope_cbor_hex,
  acceptedReceiptHex: acceptedBatch.expect.accepted_receipt_cbor_hex,
  controlGenesisHex: chain.transitions[0].committed_cbor_hex,
  controlPageHex: controlPage.toString('hex'),
  controlHeadHex: chain.transitions[1].head_hash_hex,
  batchPageHex: batchPage.toString('hex'),
  batchResultHex: batchResult.toString('hex'),
  managerDeviceHex: chain.test_only_inputs.manager_device_id_hex,
  managerSeedHex: chain.test_only_inputs.manager_sign_seed_hex,
};

const server = http.createServer((request, response) => {
  if (request.url === '/') {
    response.writeHead(200, { 'Content-Type': 'text/html' });
    response.end('<!doctype html><title>babytrack wasm storage smoke</title>');
    return;
  }
  const name = request.url.slice(1);
  if (name === 'local-store.js' || name === 'public-store.js') {
    response.writeHead(200, { 'Content-Type': 'text/javascript' });
    fs.createReadStream(path.join(__dirname, '../../core-wasm/web', name)).pipe(response);
    return;
  }
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

    const localFirst = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { LocalStore } = await import('/local-store.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g), (pair) => parseInt(pair, 16));
      const store = await LocalStore.open(wasm, 'babytrack-local-journal-smoke');
      await store.createFamily(data.familyHex, data.deviceHex);
      await store.createFamily(data.otherFamilyHex, data.deviceHex);
      const index = await store.append(data.familyHex, bytes(data.operationHex));
      let duplicateRejected = false;
      try { await store.append(data.familyHex, bytes(data.operationHex)); }
      catch { duplicateRejected = true; }
      let rejected = false;
      try { await store.append(data.otherFamilyHex, bytes(data.operationHex)); }
      catch { rejected = true; }
      const projection = await store.load(data.familyHex);
      const result = {
        index,
        name: Array.from(projection.field_cbor(bytes(data.recordHex), 1n)),
        unknown: Array.from(projection.field_cbor(bytes(data.recordHex), 500n)),
        rejected, duplicateRejected,
      };
      projection.free();
      store.close();
      return result;
    }, input);
    assert.deepEqual(localFirst, {
      index: 1, name: [0x64, 0x42, 0x61, 0x62, 0x79], unknown: [0xf4],
      rejected: true, duplicateRejected: true,
    });

    await page.reload();
    const localReload = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { LocalStore } = await import('/local-store.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g), (pair) => parseInt(pair, 16));
      const store = await LocalStore.open(wasm, 'babytrack-local-journal-smoke');
      const primary = await store.load(data.familyHex);
      const other = await store.load(data.otherFamilyHex);
      const result = {
        primaryIndex: primary.last_append_index().toString(),
        otherIndex: other.last_append_index().toString(),
        name: Array.from(primary.field_cbor(bytes(data.recordHex), 1n)),
        otherName: Array.from(other.field_cbor(bytes(data.recordHex), 1n)),
      };
      primary.free();
      other.free();
      store.close();
      return result;
    }, input);
    assert.deepEqual(localReload, {
      primaryIndex: '1', otherIndex: '0', name: [0x64, 0x42, 0x61, 0x62, 0x79], otherName: [],
    });

    const publicFirst = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g), (pair) => parseInt(pair, 16));
      const store = await PublicStore.open(wasm, 'babytrack-public-authority-smoke');
      const family = await store.begin(bytes(data.publicGenesisHex), bytes(data.relayPublicHex));
      const badReceipt = bytes(data.acceptedReceiptHex);
      badReceipt[badReceipt.length - 1] ^= 1;
      let forgedRejected = false;
      try { await store.append(family, 'batch', bytes(data.acceptedEnvelopeHex), badReceipt); }
      catch { forgedRejected = true; }
      const before = await store.load(family);
      const beforeCursor = before.last_cursor().toString();
      before.free();
      const cursor = await store.append(family, 'batch', bytes(data.acceptedEnvelopeHex),
        bytes(data.acceptedReceiptHex));
      let duplicateRejected = false;
      try { await store.append(family, 'batch', bytes(data.acceptedEnvelopeHex),
        bytes(data.acceptedReceiptHex)); }
      catch { duplicateRejected = true; }
      store.close();
      return { family, beforeCursor, cursor, forgedRejected, duplicateRejected };
    }, input);
    assert.deepEqual(publicFirst, {
      family: input.familyHex, beforeCursor: '1', cursor: 2,
      forgedRejected: true, duplicateRejected: true,
    });

    await page.reload();
    const publicReload = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const store = await PublicStore.open(wasm, 'babytrack-public-authority-smoke');
      const verifier = await store.load(data.familyHex);
      const result = { cursor: verifier.last_cursor().toString(),
        head: Array.from(verifier.head_hash(), (byte) => byte.toString(16).padStart(2, '0')).join('') };
      verifier.free();
      let otherAbsent = false;
      try { await store.load(data.otherFamilyHex); }
      catch { otherAbsent = true; }
      store.close();
      return { ...result, otherAbsent };
    }, input);
    assert.deepEqual(publicReload, {
      cursor: '2', head: genesis.expect.control_head_hex, otherAbsent: true,
    });

    const pulled = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g), (pair) => parseInt(pair, 16));
      const store = await PublicStore.open(wasm, 'babytrack-public-pull-smoke');
      const family = await store.begin(bytes(data.controlGenesisHex), bytes(data.relayPublicHex));
      const expected = `/v1/families/${family}/log?after=1`;
      const progress = await store.pull(family, bytes(data.managerDeviceHex),
        bytes(data.managerSeedHex), async (path, auth) => {
          if (path !== expected || auth.length < 64) throw new Error('Unexpected signed read');
          return bytes(data.controlPageHex);
        });
      store.close();
      return { ...progress, family };
    }, input);
    assert.deepEqual(pulled, { cursor: 2, noMoreVisible: true, family: input.familyHex });
    await page.reload();
    const pulledReload = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const store = await PublicStore.open(wasm, 'babytrack-public-pull-smoke');
      const verifier = await store.load(data.familyHex);
      const result = { cursor: verifier.last_cursor().toString(),
        head: Array.from(verifier.head_hash(), (byte) => byte.toString(16).padStart(2, '0')).join('') };
      verifier.free();
      store.close();
      return result;
    }, input);
    assert.deepEqual(pulledReload, { cursor: '2', head: input.controlHeadHex });

    const batchPulled = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g), (pair) => parseInt(pair, 16));
      const store = await PublicStore.open(wasm, 'babytrack-public-batch-pull-smoke');
      const family = await store.begin(bytes(data.publicGenesisHex), bytes(data.relayPublicHex));
      let resultRead = false;
      const progress = await store.pull(family, bytes(data.managerDeviceHex),
        bytes(data.managerSeedHex), async (path, auth) => {
          if (auth.length < 64) throw new Error('Unsigned read');
          if (path === `/v1/families/${family}/log?after=1`) return bytes(data.batchPageHex);
          if (path.startsWith(`/v1/families/${family}/batch-results/`)) {
            resultRead = true;
            return bytes(data.batchResultHex);
          }
          throw new Error('Unexpected path');
        });
      store.close();
      return { ...progress, resultRead };
    }, input);
    assert.deepEqual(batchPulled, { cursor: 2, noMoreVisible: true, resultRead: true });
    await context.close();
    console.log('browser wasm + IndexedDB local journal, signed public pull, reload, rollback, and Family isolation: OK');
  } finally {
    if (browser) await browser.close();
    await new Promise((resolve) => server.close(resolve));
  }
}

run().catch((error) => { console.error(error); process.exitCode = 1; });
