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

function validatedRows(wasm, family, device, sourceOperations) {
  const projection = new wasm.WasmLocalFamily(bytes(family), bytes(device));
  const rows = [];
  try {
    for (const source of sourceOperations) {
      const operation = Uint8Array.from(source);
      const index = rows.length + 1;
      const operationId = hex(projection.append_operation(operation, BigInt(index)));
      rows.push({ family, index, operationId, operation });
    }
    return rows;
  } finally { projection.free(); }
}

export class LocalStore {
  constructor(database, wasm) {
    this.database = database;
    this.wasm = wasm;
  }

  static async open(wasm, name = 'babytrack-local') {
    const request = indexedDB.open(name, 3);
    request.onupgradeneeded = () => {
      const database = request.result;
      if (!database.objectStoreNames.contains('families')) {
        database.createObjectStore('families', { keyPath: 'family' });
      }
      if (!database.objectStoreNames.contains('operations')) {
        const operations = database.createObjectStore('operations', { keyPath: ['family', 'index'] });
        operations.createIndex('operation-id', ['family', 'operationId'], { unique: true });
      }
      if (!database.objectStoreNames.contains('copies')) {
        database.createObjectStore('copies', { keyPath: ['sourceFamily', 'transitionId'] });
      }
      if (!database.objectStoreNames.contains('copy-deliveries')) {
        database.createObjectStore('copy-deliveries',
          { keyPath: ['sourceFamily', 'transitionId', 'deliveryId'] });
      }
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

  async removalCopy(sourceFamily, transitionId) {
    const read = this.database.transaction('copies', 'readonly');
    return requestResult(read.objectStore('copies').get([sourceFamily, transitionId]));
  }

  async createRemovalCopy(sourceFamily, transitionId, family, device, sourceOperations) {
    if (family === sourceFamily) throw new Error('Independent copy needs a new Family ID');
    const rows = validatedRows(this.wasm, family, device, sourceOperations);
    const write = this.database.transaction(['families', 'operations', 'copies'], 'readwrite');
    const done = transactionDone(write);
    const existing = await requestResult(write.objectStore('copies').get([sourceFamily, transitionId]));
    if (existing) {
      await done;
      return existing.family;
    }
    write.objectStore('families').add({ family, device, lastIndex: rows.length });
    for (const row of rows) write.objectStore('operations').add(row);
    write.objectStore('copies').add({ sourceFamily, transitionId, family });
    await done;
    return family;
  }

  // The first action from a stale tab and the private copy commit together.
  // A repeated delivery ID returns the already saved action without appending.
  createRemovalCopyWithAction(sourceFamily, transitionId, family, device,
    sourceOperations, deliveryId, prepareAction) {
    if (family === sourceFamily || !deliveryId || typeof prepareAction !== 'function') {
      throw new Error('Invalid private-copy action');
    }
    return new Promise((resolve, reject) => {
      const transaction = this.database.transaction(
        ['families', 'operations', 'copies', 'copy-deliveries'], 'readwrite');
      const families = transaction.objectStore('families');
      const operations = transaction.objectStore('operations');
      const copies = transaction.objectStore('copies');
      const deliveries = transaction.objectStore('copy-deliveries');
      let failure;
      let destination;
      transaction.oncomplete = () => resolve(destination);
      transaction.onerror = () => reject(failure || transaction.error);
      transaction.onabort = () => reject(failure || transaction.error || new Error('transaction aborted'));
      const abort = (error) => { failure = error; transaction.abort(); };

      copies.get([sourceFamily, transitionId]).onsuccess = (event) => {
        const existing = event.target.result;
        destination = existing?.family || family;
        deliveries.get([sourceFamily, transitionId, deliveryId]).onsuccess = (deliveryEvent) => {
          const delivered = deliveryEvent.target.result;
          if (delivered) {
            if (!existing || delivered.family !== existing.family) {
              abort(new Error('Private-copy delivery differs from copy mapping'));
            }
            return;
          }
          if (!existing) {
            const projection = new this.wasm.WasmLocalFamily(bytes(family), bytes(device));
            try {
              const rows = [];
              for (const source of sourceOperations) {
                const operation = Uint8Array.from(source);
                const index = rows.length + 1;
                const operationId = hex(projection.append_operation(operation, BigInt(index)));
                rows.push({ family, index, operationId, operation });
              }
              families.add({ family, device, lastIndex: rows.length });
              for (const row of rows) operations.add(row);
              copies.add({ sourceFamily, transitionId, family });
              const operation = prepareAction(projection);
              const index = rows.length + 1;
              const operationId = hex(projection.append_operation(operation, BigInt(index)));
              operations.add({ family, index, operationId, operation });
              families.put({ family, device, lastIndex: index });
              deliveries.add({ sourceFamily, transitionId, deliveryId, family, index });
            } catch (error) { abort(error); }
            finally { projection.free(); }
            return;
          }
          families.get(destination).onsuccess = (metadataEvent) => {
            const metadata = metadataEvent.target.result;
            if (!metadata) { abort(new Error('Private copy is absent')); return; }
            operations.getAll(IDBKeyRange.bound([destination, 1],
              [destination, Number.MAX_SAFE_INTEGER])).onsuccess = (rowsEvent) => {
              const projection = new this.wasm.WasmLocalFamily(bytes(destination), bytes(metadata.device));
              try {
                for (const row of rowsEvent.target.result) {
                  projection.append_operation(row.operation, BigInt(row.index));
                }
                if (projection.last_append_index() !== BigInt(metadata.lastIndex)) {
                  throw new Error('Private copy append index differs from operation log');
                }
                const operation = prepareAction(projection);
                const index = metadata.lastIndex + 1;
                if (!Number.isSafeInteger(index)) throw new Error('append index exhausted');
                const operationId = hex(projection.append_operation(operation, BigInt(index)));
                operations.add({ family: destination, index, operationId, operation });
                families.put({ ...metadata, lastIndex: index });
                deliveries.add({ sourceFamily, transitionId, deliveryId,
                  family: destination, index });
              } catch (error) { abort(error); }
              finally { projection.free(); }
            };
          };
        };
      };
    });
  }

  async createRestoredFamily(family, device, sourceOperations) {
    const rows = validatedRows(this.wasm, family, device, sourceOperations);
    const write = this.database.transaction(['families', 'operations'], 'readwrite');
    const done = transactionDone(write);
    write.objectStore('families').add({ family, device, lastIndex: rows.length });
    for (const row of rows) write.objectStore('operations').add(row);
    await done;
    return family;
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
