const { acceptedBatch, apiGenesis, assert, genesis, http, input, nativeExchangeBin, path, preCreateChild, preCreateSet, setRelay, spawnSync } = require('./support.cjs');

module.exports = async function ({ page, context, url, relay }) {
  setRelay(relay);

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
};
