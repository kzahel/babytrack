// Durable public authority bytes for a browser Family. The wasm Rust core
// verifies each signed transition and accepted batch; IndexedDB commits the
// exact bytes, cursor, and head together so reload always replays the proof.

const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
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
}
