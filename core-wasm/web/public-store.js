// Durable public authority bytes for a browser Family. The wasm Rust core
// verifies each signed transition and accepted batch; IndexedDB commits the
// exact bytes, cursor, and head together so reload always replays the proof.

const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
const bytes = (value) => Uint8Array.from(value.match(/../g) || [], (pair) => parseInt(pair, 16));
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
    const request = indexedDB.open(name, 1);
    request.onupgradeneeded = () => {
      const database = request.result;
      database.createObjectStore('families', { keyPath: 'family' });
      database.createObjectStore('entries', { keyPath: ['family', 'cursor'] });
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

  append(family, kind, bytes, receipt = null) {
    if (kind !== 'control' && kind !== 'batch') throw new Error('Unknown public entry kind');
    if (kind === 'batch' && receipt == null) throw new Error('Batch acceptance receipt is absent');
    return new Promise((resolve, reject) => {
      const transaction = this.database.transaction(['families', 'entries'], 'readwrite');
      const families = transaction.objectStore('families');
      const entries = transaction.objectStore('entries');
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
