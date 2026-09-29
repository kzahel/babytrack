// Browser transaction adapter for local-only operation bytes. The Rust wasm
// binding validates operations and computes projection state on every append.

const bytes = (hex) => Uint8Array.from(hex.match(/../g) || [], (pair) => parseInt(pair, 16));
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

export class LocalStore {
  constructor(database, wasm) {
    this.database = database;
    this.wasm = wasm;
  }

  static async open(wasm, name = 'babytrack-local') {
    const request = indexedDB.open(name, 1);
    request.onupgradeneeded = () => {
      const database = request.result;
      database.createObjectStore('families', { keyPath: 'family' });
      const operations = database.createObjectStore('operations', { keyPath: ['family', 'index'] });
      operations.createIndex('operation-id', ['family', 'operationId'], { unique: true });
    };
    return new LocalStore(await requestResult(request), wasm);
  }

  close() { this.database.close(); }

  async createFamily(family, device, initialOperation = null) {
    const projection = new this.wasm.WasmLocalFamily(bytes(family), bytes(device));
    let operationId = null;
    if (initialOperation) operationId = hex(projection.append_operation(initialOperation, 1n));
    projection.free();
    const transaction = this.database.transaction(['families', 'operations'], 'readwrite');
    const done = transactionDone(transaction);
    transaction.objectStore('families').add({ family, device, lastIndex: initialOperation ? 1 : 0 });
    if (initialOperation) transaction.objectStore('operations').add({
      family, index: 1, operationId, operation: initialOperation,
    });
    await done;
  }

  async families() {
    const transaction = this.database.transaction('families', 'readonly');
    return requestResult(transaction.objectStore('families').getAll());
  }

  async load(family) {
    const transaction = this.database.transaction(['families', 'operations'], 'readonly');
    const metadataRequest = transaction.objectStore('families').get(family);
    const rowsRequest = transaction.objectStore('operations').getAll(
      IDBKeyRange.bound([family, 1], [family, Number.MAX_SAFE_INTEGER]),
    );
    const [metadata, rows] = await Promise.all([
      requestResult(metadataRequest), requestResult(rowsRequest),
    ]);
    if (!metadata) throw new Error('Family is absent');
    const projection = new this.wasm.WasmLocalFamily(bytes(family), bytes(metadata.device));
    try {
      for (const row of rows) projection.append_operation(row.operation, BigInt(row.index));
      if (projection.last_append_index() !== BigInt(metadata.lastIndex)) {
        throw new Error('Family append index differs from operation log');
      }
      return projection;
    } catch (error) {
      projection.free();
      throw error;
    }
  }

  append(family, operation) {
    return new Promise((resolve, reject) => {
      const transaction = this.database.transaction(['families', 'operations'], 'readwrite');
      const families = transaction.objectStore('families');
      const operations = transaction.objectStore('operations');
      let failure;
      let appendedIndex;
      transaction.oncomplete = () => resolve(appendedIndex);
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
        const rowsRequest = operations.getAll(
          IDBKeyRange.bound([family, 1], [family, Number.MAX_SAFE_INTEGER]),
        );
        rowsRequest.onsuccess = () => {
          const projection = new this.wasm.WasmLocalFamily(bytes(family), bytes(metadata.device));
          try {
            for (const row of rowsRequest.result) {
              projection.append_operation(row.operation, BigInt(row.index));
            }
            if (projection.last_append_index() !== BigInt(metadata.lastIndex)) {
              throw new Error('Family append index differs from operation log');
            }
            const index = metadata.lastIndex + 1;
            if (!Number.isSafeInteger(index)) throw new Error('append index exhausted');
            const operationId = hex(projection.append_operation(operation, BigInt(index)));
            operations.add({ family, index, operationId, operation });
            families.put({ ...metadata, lastIndex: index });
            appendedIndex = index;
          } catch (error) {
            failure = error;
            transaction.abort();
          } finally {
            projection.free();
          }
        };
      };
    });
  }
}
