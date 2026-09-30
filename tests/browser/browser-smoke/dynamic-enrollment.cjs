const { assert, fs, holderBin, holderStep, path, restartRelay, server, setRelay, spawnSync, startRelay } = require('./support.cjs');

module.exports = async function ({ page, context, url, relay }) {
  let holderRelay;
  holderRelay = await startRelay(false, 6, true);
  setRelay(holderRelay);
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
};
