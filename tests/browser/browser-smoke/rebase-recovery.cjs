const { acceptedBatch, assert, chain, genesis, input, path, setRelay, startRelay } = require('./support.cjs');

module.exports = async function ({ page, context, url, relay }) {
  let crashRelay;
  crashRelay = await startRelay(true, 8);
  setRelay(crashRelay);
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
};
