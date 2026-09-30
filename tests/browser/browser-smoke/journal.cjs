const { assert, input, invitationStatus } = require('./support.cjs');

module.exports = async function ({ page, context, url, relay }) {
  const statusVector = await page.evaluate(async (data) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const bytes = (hex) => Uint8Array.from(hex.match(/../g), (pair) => parseInt(pair, 16));
    const invite = new wasm.WasmInvitation(data.invitation_fragment);
    try {
      const response = bytes(data.response_hex);
      const reason = invite.verify_status_reason(response);
      response[response.length - 1] ^= 1;
      let forgedRejected = false;
      try { invite.verify_status_reason(response); } catch { forgedRejected = true; }
      return { reason, forgedRejected,
        exactPath: invite.status_read_path().includes('/invitation-status/') };
    } finally { invite.free(); }
  }, invitationStatus.expected);
  assert.deepEqual(statusVector, { reason: 2, forgedRejected: true, exactPath: true });

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

  const atomicCopy = await page.evaluate(async () => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { LocalStore } = await import('/local-store.js');
    const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
    const source = wasm.new_local_ids();
    const destination = wasm.new_local_ids();
    const sourceFamily = hex(source.slice(0, 16));
    const family = hex(destination.slice(0, 16));
    const device = hex(destination.slice(16));
    const initial = new wasm.WasmLocalFamily(destination.slice(0, 16), destination.slice(16));
    const sourceOperations = [initial.create_family_operation(1n)];
    initial.free();
    const store = await LocalStore.open(wasm, 'babytrack-atomic-removal-smoke');
    const action = (projection) => projection.create_child_operation(
      'AtomicChild', undefined, undefined, 2n);
    let interrupted = false;
    try {
      await store.createRemovalCopyWithAction(sourceFamily, 'transition-one', family, device,
        sourceOperations, 'delivery-one', () => { throw new Error('crash window'); });
    } catch (error) { interrupted = error.message === 'crash window'; }
    const absentAfterAbort = !(await store.removalCopy(sourceFamily, 'transition-one')) &&
      !(await store.families()).some((row) => row.family === family);
    const first = await store.createRemovalCopyWithAction(sourceFamily, 'transition-one',
      family, device, sourceOperations, 'delivery-one', action);
    const duplicate = await store.createRemovalCopyWithAction(sourceFamily, 'transition-one',
      family, device, [], 'delivery-one', () => { throw new Error('duplicate prepared'); });
    const projection = await store.load(family);
    const index = projection.last_append_index().toString();
    const hasChild = projection.snapshot_json().includes('AtomicChild');
    projection.free();
    store.close();
    return { interrupted, absentAfterAbort, first, duplicate, family, index, hasChild };
  });
  assert.deepEqual(atomicCopy, {
    interrupted: true, absentAfterAbort: true, first: atomicCopy.family,
    duplicate: atomicCopy.family, family: atomicCopy.family, index: '2', hasChild: true,
  });
  await page.reload();
  const atomicReload = await page.evaluate(async (family) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { LocalStore } = await import('/local-store.js');
    const store = await LocalStore.open(wasm, 'babytrack-atomic-removal-smoke');
    const projection = await store.load(family);
    const result = { index: projection.last_append_index().toString(),
      hasChild: projection.snapshot_json().includes('AtomicChild') };
    projection.free();
    store.close();
    return result;
  }, atomicCopy.family);
  assert.deepEqual(atomicReload, { index: '2', hasChild: true });

};
