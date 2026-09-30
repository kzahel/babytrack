const { acceptedBatch, assert, chain, genesis, input, setRelay, startRelay } = require('./support.cjs');

module.exports = async function ({ page, context, url, relay }) {
  let rotationRelay;
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
  setRelay(rotationRelay);
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
};
