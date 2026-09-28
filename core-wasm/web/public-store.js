// Durable public authority bytes for a browser Family. The wasm Rust core
// verifies each signed transition and accepted batch; IndexedDB commits the
// exact bytes, cursor, and head together so reload always replays the proof.

const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
const bytes = (value) => Uint8Array.from(value.match(/../g) || [], (pair) => parseInt(pair, 16));
const sameBytes = (a, b) => a.length === b.length && a.every((byte, index) => byte === b[index]);
const requestResult = (request) => new Promise((resolve, reject) => {
  request.onsuccess = () => resolve(request.result);
  request.onerror = () => reject(request.error);
});
const transactionDone = (transaction) => new Promise((resolve, reject) => {
  transaction.oncomplete = resolve;
  transaction.onerror = () => reject(transaction.error);
  transaction.onabort = () => reject(transaction.error || new Error('transaction aborted'));
});

function replay(wasm, metadata, rows) {
  const verifier = new wasm.WasmPublicFamily(metadata.genesis, metadata.relayPublicKey);
  try {
    if (hex(verifier.family_id()) !== metadata.family) throw new Error('Family ID changed');
    for (const row of rows) {
      if (row.kind === 'control') verifier.apply_control(row.bytes);
      else if (row.kind === 'batch') verifier.apply_batch(row.bytes, row.receipt);
      else throw new Error('Unknown public entry kind');
      if (verifier.last_cursor() !== BigInt(row.cursor)) throw new Error('Public cursor gap');
    }
    if (verifier.last_cursor() !== BigInt(metadata.cursor) ||
        hex(verifier.head_hash()) !== metadata.head) {
      throw new Error('Public history differs from saved pin');
    }
    return verifier;
  } catch (error) {
    verifier.free();
    throw error;
  }
}

export class PublicStore {
  constructor(database, wasm) {
    this.database = database;
    this.wasm = wasm;
  }

  static async open(wasm, name = 'babytrack-public') {
    const request = indexedDB.open(name, 4);
    request.onupgradeneeded = () => {
      const database = request.result;
      if (!database.objectStoreNames.contains('families')) {
        database.createObjectStore('families', { keyPath: 'family' });
      }
      if (!database.objectStoreNames.contains('entries')) {
        database.createObjectStore('entries', { keyPath: ['family', 'cursor'] });
      }
      if (!database.objectStoreNames.contains('objects')) {
        database.createObjectStore('objects', { keyPath: ['family', 'objectId'] });
      }
      if (!database.objectStoreNames.contains('credentials')) {
        database.createObjectStore('credentials', { keyPath: 'family' });
      }
      if (!database.objectStoreNames.contains('outbox')) {
        database.createObjectStore('outbox', { keyPath: 'family' });
      }
    };
    return new PublicStore(await requestResult(request), wasm);
  }

  close() { this.database.close(); }

  async begin(genesis, relayPublicKey) {
    const verifier = new this.wasm.WasmPublicFamily(genesis, relayPublicKey);
    const metadata = {
      family: hex(verifier.family_id()),
      genesis: Uint8Array.from(genesis),
      relayPublicKey: Uint8Array.from(relayPublicKey),
      cursor: Number(verifier.last_cursor()),
      head: hex(verifier.head_hash()),
    };
    verifier.free();
    const transaction = this.database.transaction('families', 'readwrite');
    const done = transactionDone(transaction);
    transaction.objectStore('families').add(metadata);
    await done;
    return metadata.family;
  }

  async load(family) {
    const transaction = this.database.transaction(['families', 'entries'], 'readonly');
    const metadataRequest = transaction.objectStore('families').get(family);
    const rowsRequest = transaction.objectStore('entries').getAll(
      IDBKeyRange.bound([family, 1], [family, Number.MAX_SAFE_INTEGER]),
    );
    const [metadata, rows] = await Promise.all([
      requestResult(metadataRequest), requestResult(rowsRequest),
    ]);
    if (!metadata) throw new Error('Family is absent');
    return replay(this.wasm, metadata, rows);
  }

  async saveInitialCredential(family, deviceId, signingSeed, epochKey) {
    const publicVerifier = await this.load(family);
    try {
      const path = `/v1/families/${family}/log?after=${publicVerifier.last_cursor()}`;
      publicVerifier.sign_get(deviceId, signingSeed, path,
        crypto.getRandomValues(new Uint8Array(16)));
      const transaction = this.database.transaction('families', 'readonly');
      const metadata = await requestResult(transaction.objectStore('families').get(family));
      const initial = new this.wasm.WasmInitialFamily(
        metadata.genesis, metadata.relayPublicKey, epochKey,
      );
      initial.free();
    } finally {
      publicVerifier.free();
    }
    const transaction = this.database.transaction('credentials', 'readwrite');
    const done = transactionDone(transaction);
    transaction.objectStore('credentials').put({
      family, deviceId: Uint8Array.from(deviceId), signingSeed: Uint8Array.from(signingSeed),
      epochKey: Uint8Array.from(epochKey),
    });
    await done;
  }

  async initialCredential(family) {
    const transaction = this.database.transaction('credentials', 'readonly');
    const row = await requestResult(transaction.objectStore('credentials').get(family));
    if (!row) throw new Error('No saved initial device credential');
    return row;
  }

  async pullSaved(family, get, maxPages = 4) {
    const row = await this.initialCredential(family);
    return this.pull(family, row.deviceId, row.signingSeed, get, maxPages);
  }

  async hydrateGenesisSaved(family, get) {
    const row = await this.initialCredential(family);
    return this.hydrateGenesis(family, row.deviceId, row.signingSeed, get);
  }

  async loadInitialReadySaved(family) {
    const row = await this.initialCredential(family);
    const ready = await this.loadInitialReady(family, row.epochKey);
    try {
      const pending = await this.pendingInitial(family);
      if (pending) ready.preview_one(pending.operation, row.deviceId);
      return ready;
    } catch (error) {
      ready.free();
      throw error;
    }
  }

  async pendingInitial(family) {
    const transaction = this.database.transaction('outbox', 'readonly');
    return requestResult(transaction.objectStore('outbox').get(family));
  }

  // Stage exact signed bytes before any network request. One pending batch per
  // Family is retried byte-for-byte after a lost response or browser reload.
  async stageInitial(family, operation) {
    const pending = await this.pendingInitial(family);
    if (pending) {
      if (!sameBytes(pending.operation, operation)) {
        throw new Error('Another browser edit is pending upload');
      }
      return pending;
    }
    const credential = await this.initialCredential(family);
    const ready = await this.loadInitialReady(family, credential.epochKey);
    let envelope, cursor, head;
    try {
      envelope = ready.prepare_one(operation, credential.deviceId, credential.signingSeed);
      cursor = Number(ready.last_cursor());
      head = hex(ready.head_hash());
    } finally { ready.free(); }
    return new Promise((resolve, reject) => {
      const transaction = this.database.transaction(['families', 'outbox'], 'readwrite');
      const families = transaction.objectStore('families');
      const outbox = transaction.objectStore('outbox');
      let failure, result;
      transaction.oncomplete = () => resolve(result);
      transaction.onerror = () => reject(failure || transaction.error);
      transaction.onabort = () => reject(failure || transaction.error || new Error('transaction aborted'));
      const metadataRequest = families.get(family);
      metadataRequest.onsuccess = () => {
        const metadata = metadataRequest.result;
        if (!metadata || metadata.cursor !== cursor || metadata.head !== head) {
          failure = new Error('Family authority changed before staging');
          transaction.abort();
          return;
        }
        const pendingRequest = outbox.get(family);
        pendingRequest.onsuccess = () => {
          if (pendingRequest.result) {
            if (!sameBytes(pendingRequest.result.operation, operation)) {
              failure = new Error('Another browser edit is pending upload');
              transaction.abort();
            } else result = pendingRequest.result;
          } else {
            result = { family, operation: Uint8Array.from(operation), envelope: Uint8Array.from(envelope) };
            outbox.add(result);
          }
        };
      };
    });
  }

  async uploadInitial(family, post, get) {
    const pending = await this.pendingInitial(family);
    if (!pending) throw new Error('No pending initial batch');
    await post(`/v1/families/${family}/batches`, pending.envelope);
    const progress = await this.pullSaved(family, get);
    if (await this.pendingInitial(family)) throw new Error('Accepted batch not yet verified');
    return progress;
  }

  async hydrateGenesis(family, deviceId, signingSeed, get) {
    const transaction = this.database.transaction(['families', 'objects'], 'readonly');
    const metadataRequest = transaction.objectStore('families').get(family);
    const objectsRequest = transaction.objectStore('objects').getAll(
      IDBKeyRange.bound([family, ''], [family, 'f'.repeat(32)]),
    );
    const [metadata, stored] = await Promise.all([
      requestResult(metadataRequest), requestResult(objectsRequest),
    ]);
    if (!metadata) throw new Error('Family is absent');
    const verifier = await this.load(family);
    const present = new Set(stored.map((row) => row.objectId));
    let fetched = 0;
    try {
      const ids = this.wasm.manifest_object_ids(metadata.genesis);
      for (let offset = 0; offset < ids.length; offset += 16) {
        const id = ids.slice(offset, offset + 16);
        const objectId = hex(id);
        if (present.has(objectId)) continue;
        const path = `/v1/families/${family}/objects/${objectId}`;
        const auth = verifier.sign_get(deviceId, signingSeed, path,
          crypto.getRandomValues(new Uint8Array(16)));
        const response = await get(path, auth);
        const object = this.wasm.verified_manifest_object(metadata.genesis, id, response);
        const write = this.database.transaction('objects', 'readwrite');
        const done = transactionDone(write);
        write.objectStore('objects').add({ family, objectId, bytes: object });
        await done;
        present.add(objectId);
        fetched++;
      }
      return fetched;
    } finally {
      verifier.free();
    }
  }

  async loadInitialReady(family, epochKey) {
    const transaction = this.database.transaction(['families', 'entries', 'objects'], 'readonly');
    const metadataRequest = transaction.objectStore('families').get(family);
    const entriesRequest = transaction.objectStore('entries').getAll(
      IDBKeyRange.bound([family, 1], [family, Number.MAX_SAFE_INTEGER]),
    );
    const objectsRequest = transaction.objectStore('objects').getAll(
      IDBKeyRange.bound([family, ''], [family, 'f'.repeat(32)]),
    );
    const [metadata, rows, objects] = await Promise.all([
      requestResult(metadataRequest), requestResult(entriesRequest), requestResult(objectsRequest),
    ]);
    if (!metadata) throw new Error('Family is absent');
    const publicVerifier = replay(this.wasm, metadata, rows);
    let ready;
    try {
      ready = new this.wasm.WasmInitialFamily(metadata.genesis, metadata.relayPublicKey, epochKey);
      for (const row of objects) ready.add_object(bytes(row.objectId), row.bytes);
      ready.finish();
      for (const row of rows) {
        if (row.kind === 'control') ready.apply_control(row.bytes);
        else if (row.kind === 'batch') ready.apply_batch(row.bytes, row.receipt);
        else throw new Error('Unknown public entry kind');
      }
      if (ready.last_cursor() !== publicVerifier.last_cursor()) {
        throw new Error('Ready projection differs from verified public cursor');
      }
      return ready;
    } catch (error) {
      ready?.free();
      throw error;
    } finally {
      publicVerifier.free();
    }
  }

  append(family, kind, bytes, receipt = null) {
    if (kind !== 'control' && kind !== 'batch') throw new Error('Unknown public entry kind');
    if (kind === 'batch' && receipt == null) throw new Error('Batch acceptance receipt is absent');
    return new Promise((resolve, reject) => {
      const transaction = this.database.transaction(['families', 'entries', 'outbox'], 'readwrite');
      const families = transaction.objectStore('families');
      const entries = transaction.objectStore('entries');
      const outbox = transaction.objectStore('outbox');
      let failure;
      let nextCursor;
      transaction.oncomplete = () => resolve(nextCursor);
      transaction.onerror = () => reject(failure || transaction.error);
      transaction.onabort = () => reject(failure || transaction.error || new Error('transaction aborted'));
      const metadataRequest = families.get(family);
      metadataRequest.onsuccess = () => {
        const metadata = metadataRequest.result;
        if (!metadata) {
          failure = new Error('Family is absent');
          transaction.abort();
          return;
        }
        const pendingRequest = outbox.get(family);
        const rowsRequest = entries.getAll(
          IDBKeyRange.bound([family, 1], [family, Number.MAX_SAFE_INTEGER]),
        );
        rowsRequest.onsuccess = () => {
          let verifier;
          try {
            verifier = replay(this.wasm, metadata, rowsRequest.result);
            if (kind === 'control') verifier.apply_control(bytes);
            else verifier.apply_batch(bytes, receipt);
            nextCursor = Number(verifier.last_cursor());
            if (!Number.isSafeInteger(nextCursor) || nextCursor !== metadata.cursor + 1) {
              throw new Error('Public cursor exhausted or not contiguous');
            }
            entries.add({
              family, cursor: nextCursor, kind,
              bytes: Uint8Array.from(bytes),
              receipt: receipt == null ? null : Uint8Array.from(receipt),
            });
            families.put({ ...metadata, cursor: nextCursor, head: hex(verifier.head_hash()) });
            if (kind === 'batch' && pendingRequest.result &&
                sameBytes(pendingRequest.result.envelope, bytes)) outbox.delete(family);
          } catch (error) {
            failure = error;
            transaction.abort();
          } finally {
            verifier?.free();
          }
        };
      };
    });
  }

  // Fetch a bounded contiguous public prefix. `get` supplies response bytes
  // for an exact signed path; the Rust binding decodes each page and receipt.
  // Every accepted entry commits separately, so a later transport failure
  // leaves the verified prefix available after reload.
  async pull(family, deviceId, signingSeed, get, maxPages = 4) {
    if (!Number.isSafeInteger(maxPages) || maxPages < 1) throw new Error('Invalid page budget');
    const verifier = await this.load(family);
    let noMoreVisible = false;
    try {
      const read = (path) => get(path, verifier.sign_get(
        deviceId, signingSeed, path, crypto.getRandomValues(new Uint8Array(16)),
      ));
      for (let pageNumber = 0; pageNumber < maxPages; pageNumber++) {
        const after = verifier.last_cursor();
        const path = `/v1/families/${family}/log?after=${after}`;
        const page = new this.wasm.WasmLogPage(await read(path), bytes(family), after);
        try {
          if (page.has_more() && page.is_empty()) throw new Error('Relay claims more after an empty page');
          for (let index = 0; index < page.len(); index++) {
            const entry = page.entry_bytes(index);
            const kind = page.entry_kind(index);
            let receipt = null;
            if (kind === 2) {
              const id = hex(verifier.batch_id(entry));
              const resultPath = `/v1/families/${family}/batch-results/${id}`;
              receipt = this.wasm.accepted_batch_receipt(await read(resultPath));
            }
            const cursor = await this.append(family, kind === 1 ? 'control' : 'batch', entry, receipt);
            if (BigInt(cursor) !== page.entry_cursor(index)) throw new Error('Saved cursor differs from page');
            if (kind === 1) verifier.apply_control(entry);
            else verifier.apply_batch(entry, receipt);
          }
          if (verifier.last_cursor() !== page.next_after()) {
            throw new Error('Saved cursor differs from page end');
          }
          noMoreVisible = !page.has_more();
          if (noMoreVisible) break;
        } finally {
          page.free();
        }
      }
      return { cursor: Number(verifier.last_cursor()), noMoreVisible };
    } finally {
      verifier.free();
    }
  }
}
