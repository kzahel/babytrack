const assert = require('node:assert/strict');
const { spawn, spawnSync } = require('node:child_process');
const fs = require('node:fs');
const http = require('node:http');
const os = require('node:os');
const path = require('node:path');
const { chromium } = require('playwright');

const generatedDir = process.argv[2];
const relayBin = process.argv[3];
const nativeExchangeBin = process.argv[4];
const seedRecipientBin = process.argv[5];
const holderBin = process.argv[6];
if (!generatedDir || !relayBin || !nativeExchangeBin || !seedRecipientBin || !holderBin) {
  throw new Error('pass wasm-bindgen output, relay, exchange, seeder, and holder binaries');
}
const fixtures = JSON.parse(fs.readFileSync(path.join(__dirname, '../vectors/negative-batch-v1.json')));
const full = JSON.parse(fs.readFileSync(path.join(__dirname, '../vectors/full-wire-v1.json')));
const chain = JSON.parse(fs.readFileSync(path.join(__dirname, '../vectors/contiguous-chain-v1.json')));
const apiGenesis = JSON.parse(fs.readFileSync(path.join(__dirname, '../vectors/api-genesis-v1.json')));
const minor = fixtures.cases.find((entry) => entry.id === 'CROSSMINORBYTE01').input;
const genesis = full.cases.find((entry) => entry.id === 'GENESIS01');
const acceptedBatch = full.cases.find((entry) => entry.id === 'BATCHBYTE01');
const preCreateSet = fixtures.cases.find((entry) => entry.id === 'PRECREATEBYTE01').input.operations_hex[1];
const preCreateChild = fixtures.cases.find((entry) => entry.id === 'PRECREATEBYTE01').input.operations_hex[0];
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
const controlPrefixPage = (familyHex, controls) => Buffer.concat([
  Buffer.from([0xa6, 1, 1, 2, 0x50]), Buffer.from(familyHex, 'hex'),
  Buffer.from([3, 0, 4, 0x80 + controls.length]),
  ...controls.map((value, index) => Buffer.concat([
    Buffer.from([0x83, index + 1, 1]), cborBytes(Buffer.from(value, 'hex')),
  ])),
  Buffer.from([5, controls.length, 6, 0xf4]),
]);
const issueBytes = Buffer.from(chain.transitions[1].committed_cbor_hex, 'hex');
const controlPage = oneEntryPage(chain.test_only_inputs.family_id_hex, 1, issueBytes);
const batchPage = oneEntryPage(genesis.inputs.family_id_hex, 2,
  Buffer.from(acceptedBatch.expect.envelope_cbor_hex, 'hex'));
const batchResult = Buffer.concat([
  Buffer.from([0xa2, 1, 1, 2]),
  cborBytes(Buffer.from(acceptedBatch.expect.accepted_receipt_cbor_hex, 'hex')),
]);
const admission = chain.transitions[5];
const grantIdHex = admission.manifest.find((item) => item[0] === 4)[1];
const proofTransition = chain.transitions[4];
const fixtureClaim = chain.transitions[2];
const claimCandidateHex = Buffer.concat([
  Buffer.from([0xa2, 1]), Buffer.from(fixtureClaim.unsigned_cbor_hex, 'hex'),
  Buffer.from([2]), Buffer.from(fixtureClaim.signatures_cbor_hex, 'hex'),
]).toString('hex');
const claimCommittedResponseHex = Buffer.concat([
  Buffer.from([0xa2, 1, 1, 2]),
  cborBytes(Buffer.from(fixtureClaim.committed_cbor_hex, 'hex')),
]).toString('hex');
const proofTransitionIdHex = proofTransition.unsigned_cbor_hex.match(/0550([0-9a-f]{32})/)[1];
const proofCandidateHex = Buffer.concat([
  Buffer.from([0xa2, 1]), Buffer.from(proofTransition.unsigned_cbor_hex, 'hex'),
  Buffer.from([2]), Buffer.from(proofTransition.signatures_cbor_hex, 'hex'),
]).toString('hex');
const proofCommittedResponseHex = Buffer.concat([
  Buffer.from([0xa2, 1, 1, 2]),
  cborBytes(Buffer.from(proofTransition.committed_cbor_hex, 'hex')),
]).toString('hex');
const challengePageHex = Buffer.concat([
  Buffer.from([0xa6, 1, 1, 2, 0x50]),
  Buffer.from(chain.test_only_inputs.family_id_hex, 'hex'),
  Buffer.from([3, 3, 4, 0x81, 0x83, 4, 1]),
  cborBytes(Buffer.from(chain.transitions[3].committed_cbor_hex, 'hex')),
  Buffer.from([5, 4, 6, 0xf4]),
]).toString('hex');
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
  controlPromotionIdHex: chain.transitions[0].manifest[0][1],
  controlPromotionHex: chain.objects_by_id_hex[chain.transitions[0].manifest[0][1]],
  controlEpochKeyHex: chain.test_only_inputs.epoch_1_key_hex,
  batchPageHex: batchPage.toString('hex'),
  batchResultHex: batchResult.toString('hex'),
  managerDeviceHex: chain.test_only_inputs.manager_device_id_hex,
  managerSeedHex: chain.test_only_inputs.manager_sign_seed_hex,
  managerAgreementHex: chain.test_only_inputs.manager_agreement_seed_hex,
  recipientDeviceHex: chain.test_only_inputs.recipient_device_id_hex,
  recipientSeedHex: chain.test_only_inputs.recipient_sign_seed_hex,
  recipientAgreementHex: chain.test_only_inputs.recipient_agreement_seed_hex,
  admissionControlsHex: chain.transitions.slice(1, 6).map((row) => row.committed_cbor_hex),
  challengePrefixPageHex: controlPrefixPage(chain.test_only_inputs.family_id_hex,
    chain.transitions.slice(0, 4).map((row) => row.committed_cbor_hex)).toString('hex'),
  issuePrefixPageHex: controlPrefixPage(chain.test_only_inputs.family_id_hex,
    chain.transitions.slice(0, 2).map((row) => row.committed_cbor_hex)).toString('hex'),
  claimCandidateHex,
  claimCommittedResponseHex,
  challengePageHex,
  proofCommittedResponseHex,
  challengeObjectIdHex: chain.transitions[3].manifest.find((item) => item[0] === 2)[1],
  proofTransitionIdHex,
  proofCandidateHex,
  grantIdHex,
  recipientOperationHex: chain.batch.operation_cbor_hex,
  promotionIdHex: apiGenesis.inputs.promotion_id_hex,
};

let relayPort;
const server = http.createServer((request, response) => {
  if (request.url.startsWith('/v1/families/') && relayPort != null) {
    const upstream = http.request({
      hostname: '127.0.0.1', port: relayPort, path: request.url, method: request.method,
      headers: {
        Authorization: request.headers.authorization || '',
        'Content-Type': request.headers['content-type'] || 'application/cbor',
      },
    }, (result) => {
      response.writeHead(result.statusCode, { 'Content-Type': 'application/cbor' });
      result.pipe(response);
    });
    upstream.on('error', (error) => { response.writeHead(502).end(error.message); });
    request.pipe(upstream);
    return;
  }
  if (request.url === '/') {
    response.writeHead(200, { 'Content-Type': 'text/html' });
    response.end('<!doctype html><title>babytrack wasm storage smoke</title>');
    return;
  }
  const name = request.url.slice(1);
  if (name === 'local-store.js' || name === 'public-store.js' ||
      name === 'invitation-store.js' ||
      name === 'relay-get.js' || name === 'relay-post.js') {
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

async function startRelay(recipient = false, transitions = 6, holder = false) {
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'babytrack-browser-relay-'));
  const logPath = path.join(temporary, 'relay.log');
  const seedPath = path.join(temporary, 'seed');
  fs.writeFileSync(seedPath, holder ? Buffer.alloc(32, 0x6e) :
    Buffer.from(genesis.inputs.relay_sign_seed_hex, 'hex'));
  const reservation = http.createServer();
  await new Promise((resolve) => reservation.listen(0, '127.0.0.1', resolve));
  const port = reservation.address().port;
  await new Promise((resolve) => reservation.close(resolve));
  let fragment;
  if (holder) {
    const origin = `http://localhost:${server.address().port}`;
    const setup = spawnSync(path.resolve(holderBin), ['setup',
      path.join(temporary, 'manager.db'), path.join(temporary, 'relay.db'), origin],
    { encoding: 'utf8' });
    assert.equal(setup.status, 0, `Could not set up browser holder: ${setup.stderr}`);
    fragment = setup.stdout.trim();
  } else if (recipient) {
    const origin = `http://localhost:${server.address().port}`;
    const seeded = spawnSync(path.resolve(seedRecipientBin),
      [path.join(temporary, 'relay.db'), origin, String(transitions)],
      { encoding: 'utf8' });
    assert.equal(seeded.status, 0, `Could not seed recipient relay: ${seeded.stderr}`);
    fragment = seeded.stdout.trim();
  }
  const logFd = fs.openSync(logPath, 'a');
  const child = spawn(path.resolve(relayBin), [path.join(temporary, 'relay.db'), seedPath,
    `127.0.0.1:${port}`], { stdio: ['ignore', logFd, logFd] });
  const base = `http://127.0.0.1:${port}`;
  try {
    let ready = false;
    for (let attempt = 0; attempt < 100; attempt++) {
      if (child.exitCode != null) throw new Error('Disposable relay exited before startup');
      try { await fetch(base); ready = true; break; } catch { /* startup retry */ }
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
    if (!ready) throw new Error('Disposable relay did not start');
    if (recipient || holder) return { child, temporary, logFd, port, fragment,
      genesisHex: chain.transitions[0].committed_cbor_hex };
    const post = async (url, hexBody) => {
      const response = await fetch(base + url, {
        method: 'POST', headers: { 'Content-Type': 'application/cbor' },
        body: Buffer.from(hexBody, 'hex'),
      });
      const body = Buffer.from(await response.arrayBuffer());
      assert.equal(response.status, 200, `Relay fixture POST ${url}: ${body.toString('hex')}`);
      return body;
    };
    const stage = await post(apiGenesis.inputs.stage_path, apiGenesis.inputs.stage_body_cbor_hex);
    assert.equal(stage.toString('hex'), apiGenesis.expect.stage_response_cbor_hex);
    const committed = await post(apiGenesis.inputs.commit_path,
      apiGenesis.inputs.commit_candidate_cbor_hex);
    assert.equal(committed.subarray(0, 5).toString('hex'), 'a201010259');
    assert.equal(committed.readUInt16BE(5), committed.length - 7);
    return { child, temporary, logFd, port,
      genesisHex: committed.subarray(7).toString('hex') };
  } catch (error) {
    child.kill();
    fs.closeSync(logFd);
    fs.rmSync(temporary, { recursive: true, force: true });
    throw error;
  }
}

function holderStep(instance, mode) {
  const step = spawnSync(path.resolve(holderBin), [mode,
    path.join(instance.temporary, 'manager.db'),
    `http://127.0.0.1:${instance.port}`], { encoding: 'utf8' });
  assert.equal(step.status, 0, `Native holder ${mode} failed: ${step.stderr}`);
  return step.stdout.trim();
}

async function restartRelay(instance) {
  const stopped = new Promise((resolve) => instance.child.once('exit', resolve));
  instance.child.kill();
  await stopped;
  instance.child = spawn(path.resolve(relayBin), [
    path.join(instance.temporary, 'relay.db'), path.join(instance.temporary, 'seed'),
    `127.0.0.1:${instance.port}`,
  ], { stdio: ['ignore', instance.logFd, instance.logFd] });
  const base = `http://127.0.0.1:${instance.port}`;
  for (let attempt = 0; attempt < 100; attempt++) {
    if (instance.child.exitCode != null) throw new Error('Relay exited after restart');
    try { await fetch(base); return; } catch { /* startup retry */ }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error('Relay did not restart');
}

async function run() {
  await new Promise((resolve) => server.listen(0, 'localhost', resolve));
  let browser;
  let relay;
  let recipientRelay;
  let invitationRelay;
  let challengeRelay;
  let holderRelay;
  let rotationRelay;
  let crashRelay;
  try {
    relay = await startRelay();
    relayPort = relay.port;
    browser = await chromium.launch({ headless: true });
    const context = await browser.newContext();
    const page = await context.newPage();
    page.on('pageerror', (error) => { throw error; });
    const url = `http://localhost:${server.address().port}/`;
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
      const before = await store.historyStatus(family);
      if (!before.knownIncomplete) throw new Error('New public history was marked complete');
      const expected = `/v1/families/${family}/log?after=1`;
      const progress = await store.pull(family, bytes(data.managerDeviceHex),
        bytes(data.managerSeedHex), async (path, auth) => {
          if (path !== expected || auth.length < 64) throw new Error('Unexpected signed read');
          return bytes(data.controlPageHex);
        });
      const after = await store.historyStatus(family);
      if (after.knownIncomplete) throw new Error('Verified end of public history remained incomplete');
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
      const history = await store.historyStatus(data.familyHex);
      if (history.knownIncomplete) throw new Error('History completion did not survive reload');
      const result = { cursor: verifier.last_cursor().toString(),
        head: Array.from(verifier.head_hash(), (byte) => byte.toString(16).padStart(2, '0')).join('') };
      verifier.free();
      store.close();
      return result;
    }, input);
    assert.deepEqual(pulledReload, { cursor: '2', head: input.controlHeadHex });

    // A signed invitation does not stop the original manager's browser
    // tracker from replaying or preparing another epoch-one edit.
    const sameEpochReady = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g), (pair) => parseInt(pair, 16));
      const store = await PublicStore.open(wasm, 'babytrack-public-pull-smoke');
      await store.saveInitialCredential(data.familyHex, bytes(data.managerDeviceHex),
        bytes(data.managerSeedHex), bytes(data.controlEpochKeyHex),
        bytes(data.managerAgreementHex));
      const write = store.database.transaction('objects', 'readwrite');
      const committed = new Promise((resolve, reject) => {
        write.oncomplete = resolve;
        write.onabort = () => reject(write.error);
        write.onerror = () => reject(write.error);
      });
      for (const [objectId, objectBytes] of Object.entries(data.objects)) {
        write.objectStore('objects').put({ family: data.familyHex,
          objectId, bytes: bytes(objectBytes) });
      }
      await committed;
      const staged = await store.stageInitial(data.familyHex, bytes(data.createFamilyOperationHex));
      const ready = await store.loadInitialReadySaved(data.familyHex);
      const result = { cursor: Number(ready.last_cursor()),
        head: Array.from(ready.head_hash(), (byte) => byte.toString(16).padStart(2, '0')).join(''),
        staged: staged.envelope.length > 0,
        localRecordType: ready.record_type(bytes(data.familyHex)) };
      ready.free();
      store.close();
      return result;
    }, { ...input, objects: chain.objects_by_id_hex,
      createFamilyOperationHex: acceptedBatch.inputs.operation_cbor_hex });
    assert.deepEqual(sameEpochReady,
      { cursor: 2, head: input.controlHeadHex, staged: true, localRecordType: 'family' });

    invitationRelay = await startRelay(true, 2);
    relayPort = invitationRelay.port;
    const invitationPull = await page.evaluate(async (fragment) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { InvitationStore } = await import('/invitation-store.js');
      const { relayGet } = await import('/relay-get.js');
      const store = await InvitationStore.open(wasm, fragment, 'babytrack-invitation-smoke');
      const pulled = await store.pull(relayGet);
      const polled = await store.pull(relayGet);
      store.close();
      return { pulled, polled };
    }, invitationRelay.fragment);
    assert.deepEqual(invitationPull, {
      pulled: { cursor: 2, linkedIssue: true, hasMore: false },
      polled: { cursor: 2, linkedIssue: true, hasMore: false },
    });
    await page.reload();
    const savedClaim = await page.evaluate(async (fragment) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { InvitationStore } = await import('/invitation-store.js');
      const store = await InvitationStore.open(wasm, fragment, 'babytrack-invitation-smoke');
      const first = await store.prepareClaim();
      const retry = await store.prepareClaim();
      store.close();
      return { same: first.candidate.length === retry.candidate.length &&
        first.candidate.every((byte, index) => byte === retry.candidate[index]),
        candidateLength: first.candidate.length };
    }, invitationRelay.fragment);
    assert.equal(savedClaim.same, true);
    assert.ok(savedClaim.candidateLength > 0);
    await page.reload();
    const lostClaimResponse = await page.evaluate(async (fragment) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { InvitationStore } = await import('/invitation-store.js');
      const { relayPostControl } = await import('/relay-post.js');
      const store = await InvitationStore.open(wasm, fragment, 'babytrack-invitation-smoke');
      let lost = false;
      try {
        await store.submitClaim(async (path, candidate) => {
          await relayPostControl(path, candidate);
          throw new Error('simulated lost claim response');
        });
      } catch { lost = true; }
      const pending = await store.savedClaim();
      store.close();
      return { lost, pending: !!pending && !pending.committedResponse };
    }, invitationRelay.fragment);
    assert.deepEqual(lostClaimResponse, { lost: true, pending: true });
    await page.reload();
    const committedClaim = await page.evaluate(async (fragment) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { InvitationStore } = await import('/invitation-store.js');
      const { relayPostControl } = await import('/relay-post.js');
      const store = await InvitationStore.open(wasm, fragment, 'babytrack-invitation-smoke');
      const committed = await store.submitClaim(relayPostControl);
      const retry = await store.submitClaim(relayPostControl);
      store.close();
      return { cursor: committed.cursor, sameDevice:
        committed.deviceId.every((byte, index) => byte === retry.deviceId[index]) };
    }, invitationRelay.fragment);
    assert.deepEqual(committedClaim, { cursor: 3, sameDevice: true });
    await page.reload();
    const invitationReload = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { InvitationStore } = await import('/invitation-store.js');
      const { relayGet } = await import('/relay-get.js');
      const store = await InvitationStore.open(wasm, data.fragment, 'babytrack-invitation-smoke');
      const pending = await store.pullPending(relayGet);
      const pendingRetry = await store.pullPending(relayGet);
      const verifier = await store.load();
      const result = { cursor: Number(verifier.control_cursor()),
        linkedIssue: verifier.linked_issue() };
      verifier.free();
      let wrongOriginRejected = false;
      try {
        await InvitationStore.open(wasm, data.wrongOrigin, 'babytrack-wrong-origin-smoke');
      } catch { wrongOriginRejected = true; }
      const write = store.database.transaction('pages', 'readwrite');
      const pages = write.objectStore('pages');
      const row = await new Promise((resolve, reject) => {
        const request = pages.get([data.fragment, 0]);
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
      });
      row.bytes[0] ^= 1;
      pages.put(row);
      await new Promise((resolve, reject) => {
        write.oncomplete = resolve;
        write.onerror = () => reject(write.error);
        write.onabort = () => reject(write.error);
      });
      let tamperRejected = false;
      try { (await store.load()).free(); } catch { tamperRejected = true; }
      store.close();
      return { ...result, pending, pendingRetry, wrongOriginRejected, tamperRejected };
    }, { fragment: invitationRelay.fragment, wrongOrigin: chain.bootstrap.fragment });
    assert.deepEqual(invitationReload, { cursor: 3, linkedIssue: true,
      pending: { cursor: 3, hasMore: false },
      pendingRetry: { cursor: 3, hasMore: false },
      wrongOriginRejected: true, tamperRejected: true });
    challengeRelay = await startRelay(true, 4);
    relayPort = challengeRelay.port;
    const preparedProof = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { InvitationStore } = await import('/invitation-store.js');
      const { relayGet } = await import('/relay-get.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g),
        (pair) => parseInt(pair, 16));
      const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
      const store = await InvitationStore.open(wasm, data.fragment, 'babytrack-proof-smoke');
      const verifier = new wasm.WasmInvitation(data.fragment);
      verifier.accept_control_page(bytes(data.issuePrefixPageHex), 0n);
      verifier.accept_control_response(bytes(data.claimCommittedResponseHex),
        bytes(data.claimCandidateHex));
      const write = store.database.transaction(['invitations', 'pages', 'claims'], 'readwrite');
      const done = new Promise((resolve, reject) => {
        write.oncomplete = resolve;
        write.onerror = () => reject(write.error);
        write.onabort = () => reject(write.error);
      });
      write.objectStore('pages').add({ fragment: data.fragment, after: 0,
        bytes: bytes(data.issuePrefixPageHex) });
      write.objectStore('claims').add({ fragment: data.fragment, priorCursor: 2,
        candidate: bytes(data.claimCandidateHex),
        committedResponse: bytes(data.claimCommittedResponseHex),
        deviceId: bytes(data.recipientDeviceHex),
        signingSeed: bytes(data.recipientSeedHex),
        agreementPrivate: bytes(data.recipientAgreementHex) });
      write.objectStore('invitations').put({ fragment: data.fragment, cursor: 3,
        head: hex(verifier.head_hash()), linkedIssue: true });
      await done;
      verifier.free();
      const pulled = await store.pullPending(relayGet);
      const proof = await store.prepareProof(relayGet);
      const repeated = await store.prepareProof(relayGet);
      store.close();
      return { pulled, same: hex(proof.candidate) === hex(repeated.candidate),
        candidateLength: proof.candidate.length };
    }, { ...input, fragment: challengeRelay.fragment });
    assert.deepEqual(preparedProof.pulled, { cursor: 4, hasMore: false });
    assert.equal(preparedProof.same, true);
    assert.ok(preparedProof.candidateLength > 0);
    await page.reload();
    const lostProof = await page.evaluate(async (fragment) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { InvitationStore } = await import('/invitation-store.js');
      const { relayPostControl } = await import('/relay-post.js');
      const store = await InvitationStore.open(wasm, fragment, 'babytrack-proof-smoke');
      let lost = false;
      try {
        await store.submitProof(async (path, candidate) => {
          await relayPostControl(path, candidate);
          throw new Error('simulated lost proof response');
        });
      } catch { lost = true; }
      const pending = await store.savedProof();
      store.close();
      return { lost, pending: !!pending && !pending.committedResponse };
    }, challengeRelay.fragment);
    assert.deepEqual(lostProof, { lost: true, pending: true });
    await page.reload();
    const confirmedProof = await page.evaluate(async (fragment) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { InvitationStore } = await import('/invitation-store.js');
      const { PublicStore } = await import('/public-store.js');
      const { relayPostControl } = await import('/relay-post.js');
      const { relayGet } = await import('/relay-get.js');
      const store = await InvitationStore.open(wasm, fragment, 'babytrack-proof-smoke');
      const cursor = await store.submitProof(relayPostControl);
      const polled = await store.pullPending(relayGet);
      const verifier = await store.load();
      const replayed = Number(verifier.control_cursor());
      verifier.free();
      const publicStore = await PublicStore.open(wasm, 'babytrack-no-grant-smoke');
      let earlyActivationRejected = false;
      try { await store.activateFirstEpoch(publicStore, relayGet); }
      catch { earlyActivationRejected = true; }
      publicStore.close();
      store.close();
      return { cursor, polled, replayed, earlyActivationRejected };
    }, challengeRelay.fragment);
    assert.deepEqual(confirmedProof,
      { cursor: 5, polled: { cursor: 5, hasMore: false }, replayed: 5,
        earlyActivationRejected: true });
    holderRelay = await startRelay(false, 6, true);
    relayPort = holderRelay.port;
    const dynamicClaim = await page.evaluate(async (fragment) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { InvitationStore } = await import('/invitation-store.js');
      const { relayGet } = await import('/relay-get.js');
      const { relayPostControl } = await import('/relay-post.js');
      const store = await InvitationStore.open(wasm, fragment, 'babytrack-dynamic-holder-smoke');
      const pulled = await store.pull(relayGet);
      await store.prepareClaim();
      const committed = await store.submitClaim(relayPostControl);
      const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
      store.close();
      return { pulled, cursor: committed.cursor, device: hex(committed.deviceId) };
    }, holderRelay.fragment);
    assert.deepEqual({ pulled: dynamicClaim.pulled, cursor: dynamicClaim.cursor }, {
      pulled: { cursor: 2, linkedIssue: true, hasMore: false }, cursor: 3 });
    holderStep(holderRelay, 'challenge');
    await page.reload();
    const dynamicProof = await page.evaluate(async (fragment) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { InvitationStore } = await import('/invitation-store.js');
      const { relayGet } = await import('/relay-get.js');
      const { relayPostControl } = await import('/relay-post.js');
      const store = await InvitationStore.open(wasm, fragment, 'babytrack-dynamic-holder-smoke');
      const pulled = await store.pullPending(relayGet);
      await store.prepareProof(relayGet);
      const cursor = await store.submitProof(relayPostControl);
      store.close();
      return { pulled, cursor };
    }, holderRelay.fragment);
    assert.deepEqual(dynamicProof, { pulled: { cursor: 4, hasMore: false }, cursor: 5 });
    holderStep(holderRelay, 'grant');
    await restartRelay(holderRelay);
    await page.reload();
    const dynamicReady = await page.evaluate(async (fragment) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { InvitationStore } = await import('/invitation-store.js');
      const { PublicStore } = await import('/public-store.js');
      const { relayGet } = await import('/relay-get.js');
      const { relayPost } = await import('/relay-post.js');
      const store = await InvitationStore.open(wasm, fragment, 'babytrack-dynamic-holder-smoke');
      const publicStore = await PublicStore.open(wasm, 'babytrack-dynamic-ready-smoke');
      const pulled = await store.pullPending(relayGet);
      const ready = await store.activateFirstEpoch(publicStore, relayGet);
      publicStore.close();
      store.close();
      return { pulled, ready };
    }, holderRelay.fragment);
    assert.equal(dynamicReady.pulled.cursor, 6);
    assert.equal(dynamicReady.ready.cursor, 6);
    await page.reload();
    const dynamicReload = await page.evaluate(async (fragment) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { InvitationStore } = await import('/invitation-store.js');
      const { PublicStore } = await import('/public-store.js');
      const { relayGet } = await import('/relay-get.js');
      const store = await InvitationStore.open(wasm, fragment, 'babytrack-dynamic-holder-smoke');
      const publicStore = await PublicStore.open(wasm, 'babytrack-dynamic-ready-smoke');
      const ready = await store.activateFirstEpoch(publicStore, relayGet);
      publicStore.close();
      store.close();
      return ready;
    }, holderRelay.fragment);
    assert.deepEqual(dynamicReload, dynamicReady.ready);
    const dynamicChild = holderStep(holderRelay, 'write');
    const dynamicSync = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const { relayGet } = await import('/relay-get.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g),
        (pair) => parseInt(pair, 16));
      const store = await PublicStore.open(wasm, 'babytrack-dynamic-ready-smoke');
      await store.pullSaved(data.family, relayGet);
      const ready = await store.loadInitialReadySaved(data.family);
      const child = bytes(data.child);
      const nameCbor = ready.field_cbor(child, 1n);
      const result = { cursor: Number(ready.last_cursor()), type: ready.record_type(child),
        name: new TextDecoder().decode(nameCbor.slice(1)) };
      ready.free();
      store.close();
      return result;
    }, { family: dynamicReady.ready.family, child: dynamicChild });
    assert.deepEqual(dynamicSync,
      { cursor: 7, type: 'child', name: 'DynamicHolderChild' });
    const operationRun = spawnSync(path.resolve(holderBin), ['recipient_operation',
      dynamicReady.ready.family, dynamicClaim.device], { encoding: 'utf8' });
    assert.equal(operationRun.status, 0, `Could not encode browser operation: ${operationRun.stderr}`);
    const browserUpload = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const { relayGet } = await import('/relay-get.js');
      const { relayPost } = await import('/relay-post.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g),
        (pair) => parseInt(pair, 16));
      const store = await PublicStore.open(wasm, 'babytrack-dynamic-ready-smoke');
      await store.stageInitial(data.family, bytes(data.operation));
      const progress = await store.uploadInitial(data.family, relayPost, relayGet);
      const ready = await store.loadInitialReadySaved(data.family);
      const child = bytes(data.child);
      const nameCbor = ready.field_cbor(child, 1n);
      const result = { progress, cursor: Number(ready.last_cursor()),
        name: new TextDecoder().decode(nameCbor.slice(1)) };
      ready.free();
      store.close();
      return result;
    }, { family: dynamicReady.ready.family, operation: operationRun.stdout.trim(),
      child: 'b1b1b1b1b1b170b180b1b1b1b1b1b1b1' });
    assert.equal(browserUpload.cursor, 8);
    assert.equal(browserUpload.name, 'BrowserRecipientChild');
    assert.equal(holderStep(holderRelay, 'read'), 'browser child read');
    // The required CI browser job now checks an actual relay database and
    // captured relay logs, including the server restart before this upload.
    const marker = Buffer.from('BrowserRecipientChild');
    assert.equal(fs.readFileSync(path.join(holderRelay.temporary, 'relay.db')).includes(marker),
      false, 'plaintext child name reached relay storage');
    assert.equal(fs.readFileSync(path.join(holderRelay.temporary, 'relay.log')).includes(marker),
      false, 'plaintext child name reached relay logs');
    recipientRelay = await startRelay(true);
    relayPort = recipientRelay.port;
    const browserProof = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { relayGet } = await import('/relay-get.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g),
        (pair) => parseInt(pair, 16));
      const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
      const invitation = new wasm.WasmInvitation(data.fragment);
      invitation.accept_control_page(bytes(data.challengePrefixPageHex), 0n);
      const objectId = invitation.challenge_hpke_object_id();
      const path = `/v1/families/${data.familyHex}/objects/${hex(objectId)}`;
      const auth = invitation.sign_challenge_object_read(objectId,
        bytes(data.recipientDeviceHex), bytes(data.recipientSeedHex),
        crypto.getRandomValues(new Uint8Array(16)));
      const object = await relayGet(path, auth);
      const candidate = invitation.prepare_proof(bytes(data.recipientDeviceHex),
        bytes(data.recipientSeedHex), bytes(data.recipientAgreementHex), object,
        bytes(data.proofTransitionIdHex));
      let wrongKeyRejected = false;
      try {
        invitation.prepare_proof(bytes(data.recipientDeviceHex),
          bytes(data.recipientSeedHex), new Uint8Array(32), object,
          bytes(data.proofTransitionIdHex));
      } catch { wrongKeyRejected = true; }
      const changed = Uint8Array.from(object);
      changed[changed.length - 1] ^= 1;
      let wrongObjectRejected = false;
      try {
        invitation.prepare_proof(bytes(data.recipientDeviceHex),
          bytes(data.recipientSeedHex), bytes(data.recipientAgreementHex), changed,
          bytes(data.proofTransitionIdHex));
      } catch { wrongObjectRejected = true; }
      invitation.free();
      return { objectId: hex(objectId), candidate: hex(candidate),
        wrongKeyRejected, wrongObjectRejected };
    }, { ...input, fragment: recipientRelay.fragment });
    assert.deepEqual(browserProof, { objectId: input.challengeObjectIdHex,
      candidate: input.proofCandidateHex, wrongKeyRejected: true,
      wrongObjectRejected: true });
    const browserActivation = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { InvitationStore } = await import('/invitation-store.js');
      const { PublicStore } = await import('/public-store.js');
      const { relayGet } = await import('/relay-get.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g),
        (pair) => parseInt(pair, 16));
      const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
      const store = await InvitationStore.open(wasm, data.fragment, 'babytrack-activation-smoke');
      const verifier = new wasm.WasmInvitation(data.fragment);
      verifier.accept_control_page(bytes(data.issuePrefixPageHex), 0n);
      verifier.accept_control_response(bytes(data.claimCommittedResponseHex),
        bytes(data.claimCandidateHex));
      verifier.accept_control_page(bytes(data.challengePageHex), 3n);
      verifier.accept_control_response(bytes(data.proofCommittedResponseHex),
        bytes(data.proofCandidateHex));
      const write = store.database.transaction(['invitations', 'pages', 'claims', 'proofs'], 'readwrite');
      const done = new Promise((resolve, reject) => {
        write.oncomplete = resolve;
        write.onerror = () => reject(write.error);
        write.onabort = () => reject(write.error);
      });
      write.objectStore('pages').add({ fragment: data.fragment, after: 0,
        bytes: bytes(data.issuePrefixPageHex) });
      write.objectStore('pages').add({ fragment: data.fragment, after: 3,
        bytes: bytes(data.challengePageHex) });
      write.objectStore('claims').add({ fragment: data.fragment, priorCursor: 2,
        candidate: bytes(data.claimCandidateHex),
        committedResponse: bytes(data.claimCommittedResponseHex),
        deviceId: bytes(data.recipientDeviceHex), signingSeed: bytes(data.recipientSeedHex),
        agreementPrivate: bytes(data.recipientAgreementHex) });
      write.objectStore('proofs').add({ fragment: data.fragment, priorCursor: 4,
        candidate: bytes(data.proofCandidateHex),
        committedResponse: bytes(data.proofCommittedResponseHex) });
      write.objectStore('invitations').put({ fragment: data.fragment, cursor: 5,
        head: hex(verifier.head_hash()), linkedIssue: true });
      await done;
      verifier.free();
      const pending = await store.pullPending(relayGet);
      const publicStore = await PublicStore.open(wasm, 'babytrack-activated-browser-smoke');
      const activated = await store.activateFirstEpoch(publicStore, relayGet);
      const credential = await publicStore.initialCredential(activated.family);
      publicStore.close();
      store.close();
      return { pending, activated, device: hex(credential.deviceId),
        key: hex(credential.epochKey) };
    }, { ...input, fragment: recipientRelay.fragment });
    assert.deepEqual(browserActivation, {
      pending: { cursor: 6, hasMore: false },
      activated: { family: input.familyHex, cursor: 6 },
      device: input.recipientDeviceHex, key: input.controlEpochKeyHex,
    });
    await page.reload();
    const activationReload = await page.evaluate(async (fragment) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { InvitationStore } = await import('/invitation-store.js');
      const { PublicStore } = await import('/public-store.js');
      const { relayGet } = await import('/relay-get.js');
      const store = await InvitationStore.open(wasm, fragment, 'babytrack-activation-smoke');
      const publicStore = await PublicStore.open(wasm, 'babytrack-activated-browser-smoke');
      const result = await store.activateFirstEpoch(publicStore, relayGet);
      publicStore.close();
      store.close();
      return result;
    }, recipientRelay.fragment);
    assert.deepEqual(activationReload, { family: input.familyHex, cursor: 6 });
    const admittedBrowser = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const { relayGet } = await import('/relay-get.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g), (pair) => parseInt(pair, 16));
      const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
      const store = await PublicStore.open(wasm, 'babytrack-admitted-browser-smoke');
      const family = await store.begin(bytes(data.controlGenesisHex), bytes(data.relayPublicHex));
      for (const control of data.admissionControlsHex) await store.append(family, 'control', bytes(control));
      let shortcutRejected = false;
      try {
        await store.saveInitialCredential(family, bytes(data.recipientDeviceHex),
          bytes(data.recipientSeedHex), bytes(data.controlEpochKeyHex),
          bytes(data.recipientAgreementHex));
      } catch { shortcutRejected = true; }
      let missingGrantRejected = false;
      try {
        await store.saveAdmittedCredential(family, bytes(data.recipientDeviceHex),
          bytes(data.recipientSeedHex), bytes(data.recipientAgreementHex));
      } catch { missingGrantRejected = true; }
      const fetchedId = await store.hydrateInitialGrant(family, bytes(data.recipientDeviceHex),
        bytes(data.recipientSeedHex), relayGet);
      let wrongKeyRejected = false;
      try {
        await store.saveAdmittedCredential(family, bytes(data.recipientDeviceHex),
          bytes(data.recipientSeedHex), new Uint8Array(32));
      } catch { wrongKeyRejected = true; }
      await store.saveAdmittedCredential(family, bytes(data.recipientDeviceHex),
        bytes(data.recipientSeedHex), bytes(data.recipientAgreementHex));
      await store.hydrateControlObjectsSaved(family, relayGet);
      const credential = await store.initialCredential(family);
      const promotion = store.database.transaction('objects', 'readwrite');
      const committed = new Promise((resolve, reject) => {
        promotion.oncomplete = resolve;
        promotion.onabort = () => reject(promotion.error);
        promotion.onerror = () => reject(promotion.error);
      });
      promotion.objectStore('objects').put({ family,
        objectId: data.controlPromotionIdHex, bytes: bytes(data.controlPromotionHex) });
      await committed;
      const ready = await store.loadInitialReadySaved(family);
      const cursor = Number(ready.last_cursor());
      ready.free();
      const staged = await store.stageInitial(family, bytes(data.recipientOperationHex));
      store.close();
      return { shortcutRejected, missingGrantRejected, wrongKeyRejected, fetchedId,
        credentialDevice: hex(credential.deviceId), credentialKey: hex(credential.epochKey),
        agreementSaved: hex(credential.agreementPrivate) === data.recipientAgreementHex,
        cursor, staged: staged.envelope.length > 0 };
    }, input);
    assert.deepEqual(admittedBrowser, {
      shortcutRejected: true, missingGrantRejected: true, wrongKeyRejected: true,
      fetchedId: input.grantIdHex,
      credentialDevice: input.recipientDeviceHex, credentialKey: input.controlEpochKeyHex,
      agreementSaved: true, cursor: 6, staged: true,
    });
    await page.reload();
    const admittedReload = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const store = await PublicStore.open(wasm, 'babytrack-admitted-browser-smoke');
      const pending = await store.pendingInitial(data.familyHex);
      const ready = await store.loadInitialReadySaved(data.familyHex);
      const result = { pending: pending?.envelope.length > 0,
        cursor: Number(ready.last_cursor()), agreementSaved:
          (await store.initialCredential(data.familyHex)).agreementPrivate.length === 32 };
      ready.free();
      store.close();
      return result;
    }, input);
    assert.deepEqual(admittedReload, { pending: true, cursor: 6, agreementSaved: true });
    const admittedUpload = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const { relayGet } = await import('/relay-get.js');
      const { relayPost } = await import('/relay-post.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g), (pair) => parseInt(pair, 16));
      const store = await PublicStore.open(wasm, 'babytrack-admitted-browser-smoke');
      const progress = await store.uploadInitial(data.familyHex, relayPost, relayGet);
      const ready = await store.loadInitialReadySaved(data.familyHex);
      const result = { ...progress, pending: !!(await store.pendingInitial(data.familyHex)),
        childType: ready.record_type(bytes('0183f9d0000070008000000000000001')) };
      ready.free();
      store.close();
      return result;
    }, input);
    assert.deepEqual(admittedUpload,
      { cursor: 7, noMoreVisible: true, pending: false, childType: 'child' });
    const recipientSeenByManager = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const { relayGet } = await import('/relay-get.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g),
        (pair) => parseInt(pair, 16));
      const hex = (value) => Array.from(value,
        (byte) => byte.toString(16).padStart(2, '0')).join('');
      const store = await PublicStore.open(wasm, 'babytrack-manager-reads-recipient-smoke');
      const family = await store.begin(bytes(data.controlGenesisHex), bytes(data.relayPublicHex));
      for (const control of data.admissionControlsHex) {
        await store.append(family, 'control', bytes(control));
      }
      const objectWrite = store.database.transaction('objects', 'readwrite');
      const objectCommitted = new Promise((resolve, reject) => {
        objectWrite.oncomplete = resolve;
        objectWrite.onabort = () => reject(objectWrite.error);
        objectWrite.onerror = () => reject(objectWrite.error);
      });
      objectWrite.objectStore('objects').put({ family,
        objectId: data.controlPromotionIdHex, bytes: bytes(data.controlPromotionHex) });
      await objectCommitted;
      await store.saveInitialCredential(family, bytes(data.managerDeviceHex),
        bytes(data.managerSeedHex), bytes(data.controlEpochKeyHex),
        bytes(data.managerAgreementHex));
      const progress = await store.pullSaved(family, relayGet);
      await store.hydrateControlObjectsSaved(family, relayGet);
      const ready = await store.loadInitialReadySaved(family);
      const child = bytes('0183f9d0000070008000000000000001');
      const result = { ...progress, childType: ready.record_type(child),
        childNameCbor: hex(ready.field_cbor(child, 1n)) };
      ready.free();
      store.close();
      return result;
    }, input);
    assert.deepEqual(recipientSeenByManager,
      { cursor: 7, noMoreVisible: true, childType: 'child', childNameCbor: '6442616279' });
    relayPort = relay.port;

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

    const realRelay = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const { relayGet } = await import('/relay-get.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g), (pair) => parseInt(pair, 16));
      const store = await PublicStore.open(wasm, 'babytrack-real-relay-public-smoke');
      const family = await store.begin(bytes(data.genesisHex), bytes(data.relayPublicHex));
      const wrongKey = bytes(data.epochKeyHex);
      wrongKey[0] ^= 1;
      let wrongKeyRejected = false;
      try { await store.saveInitialCredential(family, bytes(data.managerDeviceHex),
        bytes(data.managerSeedHex), wrongKey, bytes(data.managerAgreementHex)); }
      catch { wrongKeyRejected = true; }
      let noCredentialAfterDenial = false;
      try { await store.initialCredential(family); }
      catch { noCredentialAfterDenial = true; }
      await store.saveInitialCredential(family, bytes(data.managerDeviceHex),
        bytes(data.managerSeedHex), bytes(data.epochKeyHex),
        bytes(data.managerAgreementHex));
      const progress = await store.pullSaved(family, relayGet);
      const fetched = await store.hydrateGenesisSaved(family, relayGet);
      let invalidLocalRejected = false;
      try { await store.stageInitial(family, bytes(data.invalidLocalOperationHex)); }
      catch (error) { invalidLocalRejected = error.message.includes('record does not exist'); }
      const staged = await store.stageInitial(family, bytes(data.operationHex));
      const retry = await store.stageInitial(family, bytes(data.operationHex));
      const other = bytes(data.operationHex);
      other[other.length - 1] ^= 1;
      let otherEditBlocked = false;
      try { await store.stageInitial(family, other); }
      catch (error) { otherEditBlocked = error.message === 'Another browser edit is pending upload'; }
      const localReady = await store.loadInitialReadySaved(family);
      const localRecordType = localReady.record_type(bytes(family));
      const localCursor = localReady.last_cursor().toString();
      localReady.free();
      store.close();
      return { ...progress, family, fetched, staged: staged.envelope.length > 0,
        exactRetry: staged.envelope.every((value, index) => value === retry.envelope[index]),
        invalidLocalRejected, otherEditBlocked, localRecordType, localCursor,
        wrongKeyRejected, noCredentialAfterDenial };
    }, {
      genesisHex: relay.genesisHex,
      relayPublicHex: input.relayPublicHex,
      managerDeviceHex: input.managerDeviceHex,
      managerSeedHex: input.managerSeedHex,
      managerAgreementHex: input.managerAgreementHex,
      epochKeyHex: genesis.inputs.epoch_key_hex,
      operationHex: acceptedBatch.inputs.operation_cbor_hex,
      invalidLocalOperationHex: preCreateSet.replaceAll('723e4567e89b42d3a456426614174000',
        input.managerDeviceHex),
    });
    assert.deepEqual(realRelay, {
      cursor: 1, noMoreVisible: true, family: input.familyHex, fetched: 1,
      staged: true, exactRetry: true, wrongKeyRejected: true, noCredentialAfterDenial: true,
      invalidLocalRejected: true, otherEditBlocked: true,
      localRecordType: 'family', localCursor: '1',
    });
    await page.reload();
    const uploaded = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const { relayGet } = await import('/relay-get.js');
      const { relayPost } = await import('/relay-post.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g), (pair) => parseInt(pair, 16));
      const store = await PublicStore.open(wasm, 'babytrack-real-relay-public-smoke');
      const pendingBefore = await store.pendingInitial(data.familyHex);
      const beforeReady = await store.loadInitialReadySaved(data.familyHex);
      const visibleAfterReload = beforeReady.record_type(bytes(data.familyHex));
      beforeReady.free();
      let lostResponseKept = false;
      try {
        await store.uploadInitial(data.familyHex, async (path, envelope) => {
          await relayPost(path, envelope);
          throw new Error('simulated lost response');
        }, relayGet);
      } catch (error) {
        if (error.message !== 'simulated lost response') throw error;
        const retry = await store.pendingInitial(data.familyHex);
        lostResponseKept = !!retry && retry.envelope.every(
          (value, index) => value === pendingBefore.envelope[index]);
      }
      const progress = await store.uploadInitial(data.familyHex, relayPost, relayGet);
      const ready = await store.loadInitialReadySaved(data.familyHex);
      const accepted = await new Promise((resolve, reject) => {
        const request = store.database.transaction('entries', 'readonly')
          .objectStore('entries').get([data.familyHex, 2]);
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
      });
      const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
      const verifier = await store.load(data.familyHex);
      const credential = await store.initialCredential(data.familyHex);
      const objectPath = `/v1/families/${data.familyHex}/objects/${data.promotionIdHex}`;
      const objectAuth = verifier.sign_get(credential.deviceId, credential.signingSeed,
        objectPath, crypto.getRandomValues(new Uint8Array(16)));
      const objectResponse = await relayGet(objectPath, objectAuth);
      verifier.free();
      const result = { ...progress, pendingBefore: !!pendingBefore, lostResponseKept,
        visibleAfterReload,
        pendingAfter: !!(await store.pendingInitial(data.familyHex)),
        recordType: ready.record_type(bytes(data.familyHex)),
        nativeEnvelopeHex: hex(accepted.bytes), nativeReceiptHex: hex(accepted.receipt),
        nativeObjectHex: hex(objectResponse) };
      ready.free();
      store.close();
      return result;
    }, input);
    const { nativeEnvelopeHex, nativeReceiptHex, nativeObjectHex, ...uploadedStatus } = uploaded;
    assert.deepEqual(uploadedStatus, { cursor: 2, noMoreVisible: true,
      pendingBefore: true, lostResponseKept: true, visibleAfterReload: 'family',
      pendingAfter: false, recordType: 'family' });
    const native = spawnSync(path.resolve(nativeExchangeBin), [
      relay.genesisHex, input.relayPublicHex, genesis.inputs.epoch_key_hex,
      input.managerSeedHex, apiGenesis.inputs.promotion_id_hex,
      nativeObjectHex, nativeEnvelopeHex, nativeReceiptHex,
    ], { encoding: 'utf8', maxBuffer: 1024 * 1024 });
    assert.equal(native.status, 0, `Native browser exchange: ${native.stderr}`);
    const [nativeEnvelopeHex2, nativeChildHex] = native.stdout.trim().split(' ');
    assert.match(nativeEnvelopeHex2, /^[0-9a-f]+$/);
    assert.match(nativeChildHex, /^[0-9a-f]{32}$/);
    const nativeUpload = await fetch(`http://127.0.0.1:${relay.port}/v1/families/${input.familyHex}/batches`, {
      method: 'POST', headers: { 'Content-Type': 'application/cbor' },
      body: Buffer.from(nativeEnvelopeHex2, 'hex'),
    });
    assert.equal(nativeUpload.status, 200,
      `Native batch upload: ${Buffer.from(await nativeUpload.arrayBuffer()).toString('hex')}`);
    const nativeInBrowser = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const { relayGet } = await import('/relay-get.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g), (pair) => parseInt(pair, 16));
      const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
      const store = await PublicStore.open(wasm, 'babytrack-real-relay-public-smoke');
      const progress = await store.pullSaved(data.familyHex, relayGet);
      const ready = await store.loadInitialReadySaved(data.familyHex);
      const result = { ...progress, childType: ready.record_type(bytes(data.childHex)),
        childNameCbor: hex(ready.field_cbor(bytes(data.childHex), 1n)) };
      ready.free();
      store.close();
      return result;
    }, { familyHex: input.familyHex, childHex: nativeChildHex });
    assert.deepEqual(nativeInBrowser, { cursor: 3, noMoreVisible: true,
      childType: 'child', childNameCbor: '704d69786564436c69656e744368696c64' });
    await page.reload();
    const relayReload = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const store = await PublicStore.open(wasm, 'babytrack-real-relay-public-smoke');
      const bytes = (value) => Uint8Array.from(value.match(/../g), (pair) => parseInt(pair, 16));
      const ready = await store.loadInitialReadySaved(data.familyHex);
      const result = { cursor: ready.last_cursor().toString(),
        recordType: ready.record_type(bytes(data.familyHex)),
        childType: ready.record_type(bytes(data.childHex)) };
      ready.free();
      store.close();
      return result;
    }, { familyHex: input.familyHex, childHex: nativeChildHex });
    assert.deepEqual(relayReload, { cursor: '3', recordType: 'family', childType: 'child' });
    const queuedChild = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const { relayGet } = await import('/relay-get.js');
      const { relayPost } = await import('/relay-post.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g), (pair) => parseInt(pair, 16));
      const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
      const store = await PublicStore.open(wasm, 'babytrack-real-relay-public-smoke');
      await store.stageInitial(data.familyHex, bytes(data.createChildHex));
      const queue = await store.queueInitial(data.familyHex, bytes(data.renameChildHex));
      const ready = await store.loadInitialReadySaved(data.familyHex);
      const result = { ...queue, cursor: Number(ready.last_cursor()),
        childType: ready.record_type(bytes(data.childHex)),
        childNameCbor: hex(ready.field_cbor(bytes(data.childHex), 1n)) };
      ready.free();
      const pending = await store.pendingInitial(data.familyHex);
      await relayPost(`/v1/families/${data.familyHex}/batches`, pending.envelope);
      const firstAccepted = await store.pullSaved(data.familyHex, relayGet);
      result.acceptedCursor = firstAccepted.cursor;
      result.afterAcceptance = await store.queueInitial(data.familyHex, bytes(data.renameAgainHex));
      store.close();
      return result;
    }, { familyHex: input.familyHex, childHex: '0183f9d0000070008000000000000021',
      createChildHex: preCreateChild.replaceAll('723e4567e89b42d3a456426614174000', input.managerDeviceHex),
      renameChildHex: preCreateSet
        .replaceAll('723e4567e89b42d3a456426614174000', input.managerDeviceHex)
        .replace('0183f9d0000070008000000000000022', '0183f9d0000070008000000000000021'),
      renameAgainHex: preCreateSet
        .replaceAll('723e4567e89b42d3a456426614174000', input.managerDeviceHex)
        .replace('0183f9d0000070008000000000000022', '0183f9d0000070008000000000000021')
        .replace('0183f9d0000070008000000000000024', '0183f9d0000070008000000000000025')
        .replace('08831b000001a0dd591dc80050', '08831b000001a0dd591dc80150')
        .replace('66467574757265', '664c6174657221') });
    assert.deepEqual(queuedChild, { pending: true, queued: 1, cursor: 3,
      childType: 'child', childNameCbor: '66467574757265', acceptedCursor: 4,
      afterAcceptance: { pending: false, queued: 2 } });
    await page.reload();
    const lostQueuedResponse = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const { relayGet } = await import('/relay-get.js');
      const { relayPost } = await import('/relay-post.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g), (pair) => parseInt(pair, 16));
      const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
      const store = await PublicStore.open(wasm, 'babytrack-real-relay-public-smoke');
      const before = await store.loadInitialReadySaved(data.familyHex);
      const visibleBefore = hex(before.field_cbor(bytes(data.childHex), 1n));
      before.free();
      const queuedBefore = (await store.queuedInitial(data.familyHex)).length;
      let responseLost = false;
      try {
        await store.uploadInitial(data.familyHex, async (path, body) => {
          await relayPost(path, body);
          throw new Error('Simulated lost queued batch response');
        }, relayGet);
      } catch (error) {
        responseLost = error.message === 'Simulated lost queued batch response';
        if (!responseLost) throw error;
      }
      const pending = await store.pendingInitial(data.familyHex);
      const queue = await store.queuedInitial(data.familyHex);
      const ready = await store.loadInitialReadySaved(data.familyHex);
      const result = { queuedBefore, visibleBefore, responseLost,
        pendingAfterLost: !!pending, queuedAfterLost: queue.length,
        cursorAfterLost: Number(ready.last_cursor()),
        childNameCbor: hex(ready.field_cbor(bytes(data.childHex), 1n)),
        exactEnvelope: pending && hex(pending.envelope) };
      ready.free();
      store.close();
      return result;
    }, { familyHex: input.familyHex, childHex: '0183f9d0000070008000000000000021' });
    const { exactEnvelope, ...lostQueuedStatus } = lostQueuedResponse;
    assert.match(exactEnvelope, /^[0-9a-f]+$/);
    assert.deepEqual(lostQueuedStatus, { queuedBefore: 2,
      visibleBefore: '664c6174657221', responseLost: true,
      pendingAfterLost: true, queuedAfterLost: 1, cursorAfterLost: 4,
      childNameCbor: '664c6174657221' });
    await page.reload();
    const drainedQueue = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const { relayGet } = await import('/relay-get.js');
      const { relayPost } = await import('/relay-post.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g), (pair) => parseInt(pair, 16));
      const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
      const store = await PublicStore.open(wasm, 'babytrack-real-relay-public-smoke');
      const before = await store.loadInitialReadySaved(data.familyHex);
      const visibleBefore = hex(before.field_cbor(bytes(data.childHex), 1n));
      before.free();
      const pendingBefore = await store.pendingInitial(data.familyHex);
      const queuedBefore = (await store.queuedInitial(data.familyHex)).length;
      const progress = await store.uploadInitial(data.familyHex, relayPost, relayGet);
      const ready = await store.loadInitialReadySaved(data.familyHex);
      const result = { ...progress, queuedBefore, visibleBefore,
        retriedExact: pendingBefore && hex(pendingBefore.envelope) === data.exactEnvelope,
        pendingAfter: !!(await store.pendingInitial(data.familyHex)),
        queuedAfter: (await store.queuedInitial(data.familyHex)).length,
        childNameCbor: hex(ready.field_cbor(bytes(data.childHex), 1n)) };
      ready.free();
      store.close();
      return result;
    }, { familyHex: input.familyHex, childHex: '0183f9d0000070008000000000000021',
      exactEnvelope });
    assert.deepEqual(drainedQueue, { cursor: 6, noMoreVisible: true, queuedBefore: 1,
      visibleBefore: '664c6174657221', retriedExact: true,
      pendingAfter: false, queuedAfter: 0, childNameCbor: '664c6174657221' });
    // FS48: verified rotation and its keyring survive an IndexedDB reopen.
    const rotationSaved = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g), (pair) => parseInt(pair, 16));
      const store = await PublicStore.open(wasm, 'babytrack-rotation-smoke');
      const family = await store.begin(bytes(data.genesis), bytes(data.relayPublic));
      await store.saveInitialCredential(family, bytes(data.device), bytes(data.seed),
        bytes(data.key), bytes(data.agreement));
      const write = store.database.transaction('objects', 'readwrite');
      const done = new Promise((resolve, reject) => {
        write.oncomplete = resolve;
        write.onabort = () => reject(write.error);
      });
      for (const [id, object] of Object.entries(data.objects)) {
        write.objectStore('objects').add({ family, objectId: id, bytes: bytes(object) });
      }
      await done;
      for (const control of data.controls.slice(1, 7)) {
        await store.append(family, 'control', bytes(control));
      }
      await store.append(family, 'batch', bytes(data.envelope), bytes(data.receipt));
      await store.append(family, 'control', bytes(data.controls[7]));
      const ready = await store.loadInitialReadySaved(family);
      const result = { family, cursor: Number(ready.last_cursor()),
        type: ready.record_type(bytes(data.child)) };
      ready.free();
      store.close();
      return result;
    }, {
      genesis: chain.transitions[0].committed_cbor_hex,
      relayPublic: genesis.expect.relay_public_key_hex,
      device: input.managerDeviceHex, seed: input.managerSeedHex,
      agreement: input.managerAgreementHex, key: input.controlEpochKeyHex,
      controls: chain.transitions.map((row) => row.committed_cbor_hex),
      envelope: chain.batch.envelope_cbor_hex,
      receipt: chain.batch.receipt_cbor_hex,
      objects: chain.objects_by_id_hex,
      child: '0183f9d0000070008000000000000001',
    });
    assert.deepEqual(rotationSaved, { family: chain.test_only_inputs.family_id_hex,
      cursor: 9, type: 'child' });
    await page.reload();
    const rotationReopened = await page.evaluate(async (family) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const store = await PublicStore.open(wasm, 'babytrack-rotation-smoke');
      const ready = await store.loadInitialReadySaved(family);
      const result = Number(ready.last_cursor());
      ready.free();
      store.close();
      return result;
    }, rotationSaved.family);
    assert.equal(rotationReopened, 9);
    rotationRelay = await startRelay(true, 8);
    relayPort = rotationRelay.port;
    const liveRotation = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const { relayGet } = await import('/relay-get.js');
      const { relayPost } = await import('/relay-post.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g),
        (pair) => parseInt(pair, 16));
      const store = await PublicStore.open(wasm, 'babytrack-live-rotation-smoke');
      const family = await store.begin(bytes(data.genesis), bytes(data.relayPublic));
      await store.saveInitialCredential(family, bytes(data.device), bytes(data.seed),
        bytes(data.key), bytes(data.agreement));
      // Stage while the browser knows only epoch one. The relay has already
      // rotated, so these exact bytes must receive a signed stale rejection.
      const genesisObjects = await store.hydrateGenesisSaved(family, relayGet);
      const stale = await store.stageInitial(family, bytes(data.operation));
      const progress = await store.pullSaved(family, relayGet);
      const controlObjects = await store.hydrateControlObjectsSaved(family, relayGet);
      const ready = await store.loadInitialReadySaved(family);
      const result = { ...progress, genesisObjects, controlObjects,
        readyCursor: Number(ready.last_cursor()),
        child: ready.record_type(bytes(data.child)) };
      ready.free();
      await relayPost(`/v1/families/${family}/batches`, stale.envelope);
      const verifier = await store.load(family);
      const resultPath = `/v1/families/${family}/batch-results/${Array.from(verifier.batch_id(stale.envelope),
        (value) => value.toString(16).padStart(2, '0')).join('')}`;
      const auth = verifier.sign_get(bytes(data.device), bytes(data.seed), resultPath,
        crypto.getRandomValues(new Uint8Array(16)));
      const tampered = Uint8Array.from(await relayGet(resultPath, auth));
      tampered[tampered.length - 1] ^= 1;
      result.tamperKept = !(await store.rebaseRejectedStaleInitial(family,
        async () => tampered)) && !!(await store.pendingInitial(family));
      verifier.free();
      let exhausted = false;
      try { await store.uploadInitial(family, relayPost, relayGet, 1); }
      catch (error) { exhausted = error.message.includes('batch budget exhausted'); }
      result.budgetKept = exhausted && !(await store.pendingInitial(family)) &&
        (await store.queuedInitial(family)).length === 1;
      const upload = await store.uploadInitial(family, relayPost, relayGet);
      const after = await store.loadInitialReadySaved(family);
      result.uploadCursor = upload.cursor;
      result.afterCursor = Number(after.last_cursor());
      result.managerRecord = after.record_type(bytes(family));
      result.resealed = !(await store.pendingInitial(family)) &&
        stale.envelope.length > 0 && (await store.queuedInitial(family)).length === 0;
      after.free();
      store.close();
      return result;
    }, {
      genesis: chain.transitions[0].committed_cbor_hex,
      relayPublic: genesis.expect.relay_public_key_hex,
      device: input.managerDeviceHex, seed: input.managerSeedHex,
      agreement: input.managerAgreementHex, key: input.controlEpochKeyHex,
      child: '0183f9d0000070008000000000000001',
      operation: acceptedBatch.inputs.operation_cbor_hex,
    });
    assert.deepEqual(liveRotation, { cursor: 9, noMoreVisible: true,
      genesisObjects: 1, controlObjects: 10, readyCursor: 9, child: 'child',
      uploadCursor: 10, afterCursor: 10, managerRecord: 'family', resealed: true,
      tamperKept: true, budgetKept: true });
    await page.reload();
    const liveRotationReload = await page.evaluate(async () => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const store = await PublicStore.open(wasm, 'babytrack-live-rotation-smoke');
      const families = await new Promise((resolve, reject) => {
        const request = store.database.transaction('families').objectStore('families').getAll();
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
      });
      const ready = await store.loadInitialReadySaved(families[0].family);
      const cursor = Number(ready.last_cursor());
      ready.free();
      store.close();
      return cursor;
    });
    assert.equal(liveRotationReload, 10);
    crashRelay = await startRelay(true, 8);
    relayPort = crashRelay.port;
    const crashInput = {
      genesis: chain.transitions[0].committed_cbor_hex,
      relayPublic: genesis.expect.relay_public_key_hex,
      device: input.managerDeviceHex, seed: input.managerSeedHex,
      agreement: input.managerAgreementHex, key: input.controlEpochKeyHex,
      operation: acceptedBatch.inputs.operation_cbor_hex,
    };
    const crashedAfterRebase = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const { relayGet } = await import('/relay-get.js');
      const { relayPost } = await import('/relay-post.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g),
        (pair) => parseInt(pair, 16));
      const store = await PublicStore.open(wasm, 'babytrack-stale-crash-smoke');
      const family = await store.begin(bytes(data.genesis), bytes(data.relayPublic));
      await store.saveInitialCredential(family, bytes(data.device), bytes(data.seed),
        bytes(data.key), bytes(data.agreement));
      await store.hydrateGenesisSaved(family, relayGet);
      const staged = await store.stageInitial(family, bytes(data.operation));
      store.hydrateControlObjectsSaved = async () => {
        throw new Error('simulated crash after stale rebase');
      };
      let interrupted = false;
      try { await store.uploadInitial(family, relayPost, relayGet); }
      catch (error) { interrupted = error.message === 'simulated crash after stale rebase'; }
      const verifier = await store.load(family);
      const oldId = Array.from(verifier.batch_id(staged.envelope),
        (value) => value.toString(16).padStart(2, '0')).join('');
      const cursor = Number(verifier.last_cursor());
      verifier.free();
      const result = { family, oldId, interrupted,
        cursor,
        pending: !!(await store.pendingInitial(family)),
        queued: (await store.queuedInitial(family)).length };
      store.close();
      return result;
    }, crashInput);
    assert.deepEqual({ ...crashedAfterRebase, family: undefined, oldId: undefined },
      { family: undefined, oldId: undefined, interrupted: true, cursor: 9,
        pending: false, queued: 1 });
    await page.reload();
    const partialHydration = await page.evaluate(async (family) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const { relayGet } = await import('/relay-get.js');
      const { relayPost } = await import('/relay-post.js');
      const store = await PublicStore.open(wasm, 'babytrack-stale-crash-smoke');
      let objectReads = 0;
      let interrupted = false;
      try {
        await store.uploadInitial(family, relayPost, (path, auth) => {
          if (path.includes('/objects/') && ++objectReads === 2) {
            throw new Error('simulated partial hydration');
          }
          return relayGet(path, auth);
        });
      } catch (error) { interrupted = error.message === 'simulated partial hydration'; }
      const result = { interrupted, objectReads,
        pending: !!(await store.pendingInitial(family)),
        queued: (await store.queuedInitial(family)).length };
      store.close();
      return result;
    }, crashedAfterRebase.family);
    assert.deepEqual(partialHydration, { interrupted: true, objectReads: 2,
      pending: false, queued: 1 });
    await page.reload();
    const recoveredStale = await page.evaluate(async (data) => {
      const wasm = await import('/babytrack_core_wasm.js');
      await wasm.default('/babytrack_core_wasm_bg.wasm');
      const { PublicStore } = await import('/public-store.js');
      const { relayGet } = await import('/relay-get.js');
      const { relayPost } = await import('/relay-post.js');
      const bytes = (value) => Uint8Array.from(value.match(/../g),
        (pair) => parseInt(pair, 16));
      const store = await PublicStore.open(wasm, 'babytrack-stale-crash-smoke');
      const progress = await store.uploadInitial(data.family, relayPost, relayGet);
      const ready = await store.loadInitialReadySaved(data.family);
      const verifier = await store.load(data.family);
      const accepted = await new Promise((resolve, reject) => {
        const request = store.database.transaction('entries').objectStore('entries')
          .get([data.family, 10]);
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
      });
      const newId = Array.from(verifier.batch_id(accepted.bytes),
        (value) => value.toString(16).padStart(2, '0')).join('');
      const result = { ...progress, cursor: Number(ready.last_cursor()),
        record: ready.record_type(bytes(data.family)),
        pending: !!(await store.pendingInitial(data.family)),
        queued: (await store.queuedInitial(data.family)).length,
        newBatch: newId !== data.oldId };
      ready.free();
      verifier.free();
      store.close();
      return result;
    }, crashedAfterRebase);
    assert.deepEqual(recoveredStale, { cursor: 10, noMoreVisible: true,
      record: 'family', pending: false, queued: 0, newBatch: true });
    await context.close();
    console.log('browser wasm + IndexedDB journal, queued offline edits, bidirectional native/browser encrypted exchange, durable retry, reload, rollback, and Family isolation: OK');
  } finally {
    if (browser) await browser.close();
    await new Promise((resolve) => server.close(resolve));
    for (const instance of [crashRelay, rotationRelay, recipientRelay, holderRelay, challengeRelay, invitationRelay, relay]) {
      if (!instance) continue;
      if (instance.child.exitCode == null) {
        const stopped = new Promise((resolve) => instance.child.once('exit', resolve));
        instance.child.kill();
        await stopped;
      }
      fs.closeSync(instance.logFd);
      fs.rmSync(instance.temporary, { recursive: true, force: true });
    }
  }
}

run().catch((error) => { console.error(error); process.exitCode = 1; });
