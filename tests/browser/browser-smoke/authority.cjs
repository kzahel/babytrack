const { acceptedBatch, assert, chain, emptyPage, genesis, input, path } = require('./support.cjs');

module.exports = async function ({ page, context, url, relay }) {
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

  const partialHistory = await page.evaluate(async (data) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { PublicStore } = await import('/public-store.js');
    const bytes = (hex) => Uint8Array.from(hex.match(/../g), (pair) => parseInt(pair, 16));
    const store = await PublicStore.open(wasm, 'babytrack-public-partial-smoke');
    const family = await store.begin(bytes(data.genesis), bytes(data.relay));
    const partial = bytes(data.page);
    partial[partial.length - 1] = 0xf5;
    const first = await store.pull(family, bytes(data.device), bytes(data.seed),
      async () => partial, 1);
    const gap = await store.historyStatus(family);
    const second = await store.pull(family, bytes(data.device), bytes(data.seed),
      async () => bytes(data.empty), 1);
    const complete = await store.historyStatus(family);
    store.close();
    return { first, gap: gap.knownIncomplete, second, complete: complete.knownIncomplete };
  }, { genesis: input.controlGenesisHex, relay: input.relayPublicHex,
    page: input.controlPageHex, device: input.managerDeviceHex,
    seed: input.managerSeedHex,
    empty: emptyPage(input.familyHex, 2).toString('hex') });
  assert.deepEqual(partialHistory, {
    first: { cursor: 2, noMoreVisible: false }, gap: true,
    second: { cursor: 2, noMoreVisible: true }, complete: false,
  });

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

};
