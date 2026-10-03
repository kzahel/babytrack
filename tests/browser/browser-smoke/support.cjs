const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const spawnSync = (command, args, options = {}) => require('node:child_process').spawnSync(command, args, { timeout: 60000, ...options });
const fs = require('node:fs');
const http = require('node:http');
const os = require('node:os');
const path = require('node:path');
const { chromium } = require('playwright');

const browserRoot = path.resolve(__dirname, '..');
const generatedDir = process.argv[2];
const relayBin = process.argv[3];
const nativeExchangeBin = process.argv[4];
const seedRecipientBin = process.argv[5];
const holderBin = process.argv[6];
const fixtureRelayBin = process.argv[7];
if (!generatedDir || !relayBin || !nativeExchangeBin || !seedRecipientBin || !holderBin || !fixtureRelayBin) {
  throw new Error('pass wasm-bindgen output, relay, exchange, seeder, holder, and fixture relay binaries');
}
const fixtures = JSON.parse(fs.readFileSync(path.join(browserRoot, '../vectors/negative-batch-v1.json')));
const full = JSON.parse(fs.readFileSync(path.join(browserRoot, '../vectors/full-wire-v1.json')));
const chain = JSON.parse(fs.readFileSync(path.join(browserRoot, '../vectors/contiguous-chain-v1.json')));
const apiGenesis = JSON.parse(fs.readFileSync(path.join(browserRoot, '../vectors/api-genesis-v1.json')));
const invitationStatus = JSON.parse(fs.readFileSync(path.join(browserRoot, '../vectors/invitation-status-v1.json')));
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
const emptyPage = (familyHex, after) => Buffer.concat([
  Buffer.from([0xa6, 1, 1, 2, 0x50]), Buffer.from(familyHex, 'hex'),
  Buffer.from([3, after, 4, 0x80, 5, after, 6, 0xf4]),
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
    fs.createReadStream(path.join(browserRoot, '../../core-wasm/web', name)).pipe(response);
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

async function createRelay(recipient = false, transitions = 6, holder = false) {
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
  const binary = recipient ? fixtureRelayBin : relayBin;
  const child = spawn(path.resolve(binary), [path.join(temporary, 'relay.db'), seedPath,
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
    await stopChild(child);
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

async function raceCandidate(firstPage, secondPage, fragment, kind) {
  const prefix = `babytrack-${kind}-race-${Date.now()}`;
  const refresh = (page, own, other) => page.evaluate(async (data) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { InvitationStore } = await import('/invitation-store.js');
    const store = await InvitationStore.open(wasm, data.fragment, 'babytrack-cas-smoke');
    try {
      const candidate = await store.refreshCandidate(data.kind, async () => {
        localStorage.setItem(data.own, 'ready');
        for (let index = 0; index < 500 && !localStorage.getItem(data.other); index++) {
          await new Promise((resolve) => setTimeout(resolve, 10));
        }
        if (!localStorage.getItem(data.other)) throw new Error('Other tab did not reach CAS barrier');
      });
      return Array.from(candidate.candidate);
    } finally { store.close(); }
  }, { fragment, kind, own, other });
  try {
    const [left, right] = await Promise.all([
      refresh(firstPage, `${prefix}-a`, `${prefix}-b`),
      refresh(secondPage, `${prefix}-b`, `${prefix}-a`),
    ]);
    return { left, right };
  } finally {
    await firstPage.evaluate((name) => {
      localStorage.removeItem(`${name}-a`);
      localStorage.removeItem(`${name}-b`);
    }, prefix);
  }
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


const relays = [];
function setRelay(instance) { relayPort = instance.port; }
async function startRelay(...args) {
  const relay = await createRelay(...args);
  relays.push(relay);
  return relay;
}
async function stopChild(child) {
  if (child.exitCode != null || child.signalCode != null) return;
  const stopped = new Promise((resolve) => child.once('exit', resolve));
  child.kill();
  const timeout = setTimeout(() => child.kill('SIGKILL'), 5000);
  try { await stopped; } finally { clearTimeout(timeout); }
}
async function disposeRelays() {
  relayPort = undefined;
  for (const relay of relays.splice(0).reverse()) {
    await stopChild(relay.child);
    fs.closeSync(relay.logFd);
    fs.rmSync(relay.temporary, { recursive: true, force: true });
  }
}
function saveRelayLogs(directory) {
  for (const [index, relay] of relays.entries()) {
    fs.copyFileSync(path.join(relay.temporary, 'relay.log'), path.join(directory, `relay-${index}.log`));
  }
}

module.exports = { acceptedBatch, apiGenesis, assert, chain, challengePageHex, chromium, claimCandidateHex, claimCommittedResponseHex, disposeRelays, emptyPage, fs, genesis, grantIdHex, holderBin, holderStep, http, input, invitationStatus, nativeExchangeBin, path, preCreateChild, preCreateSet, proofCandidateHex, proofCommittedResponseHex, proofTransitionIdHex, raceCandidate, restartRelay, saveRelayLogs, server, setRelay, spawnSync, startRelay };
