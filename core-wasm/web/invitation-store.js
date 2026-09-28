// An invitation can read public controls before it has a device credential.
// Persist the exact pages and replay them through the Rust verifier on load.
const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
const result = (request) => new Promise((resolve, reject) => {
  request.onsuccess = () => resolve(request.result);
  request.onerror = () => reject(request.error);
});
const completed = (transaction) => new Promise((resolve, reject) => {
  transaction.oncomplete = resolve;
  transaction.onabort = () => reject(transaction.error || new Error('transaction aborted'));
});

export class InvitationStore {
  constructor(database, wasm, fragment) {
    this.database = database;
    this.wasm = wasm;
    this.fragment = fragment;
  }

  static async open(wasm, fragment, name = 'babytrack-invitations') {
    const probe = new wasm.WasmInvitation(fragment);
    try {
      if (probe.relay_origin() !== location.origin) {
        throw new Error('Invitation relay origin differs from this app');
      }
    } finally { probe.free(); }
    const request = indexedDB.open(name, 1);
    request.onupgradeneeded = () => {
      const database = request.result;
      database.createObjectStore('invitations', { keyPath: 'fragment' });
      database.createObjectStore('pages', { keyPath: ['fragment', 'after'] });
    };
    return new InvitationStore(await result(request), wasm, fragment);
  }

  close() { this.database.close(); }

  async load() {
    const read = this.database.transaction(['invitations', 'pages'], 'readonly');
    const metadataRequest = read.objectStore('invitations').get(this.fragment);
    const pagesRequest = read.objectStore('pages').getAll(
      IDBKeyRange.bound([this.fragment, 0], [this.fragment, Number.MAX_SAFE_INTEGER]),
    );
    const [metadata, pages] = await Promise.all([result(metadataRequest), result(pagesRequest)]);
    const verifier = new this.wasm.WasmInvitation(this.fragment);
    try {
      for (const page of pages) {
        if (Number(verifier.control_cursor()) !== page.after) {
          throw new Error('Saved invitation page order differs');
        }
        verifier.accept_control_page(page.bytes, BigInt(page.after));
      }
      if (metadata && (Number(verifier.control_cursor()) !== metadata.cursor ||
          hex(verifier.head_hash()) !== metadata.head ||
          verifier.linked_issue() !== metadata.linkedIssue)) {
        throw new Error('Saved invitation prefix differs from pin');
      }
      if (!metadata && pages.length) throw new Error('Invitation pages lack pin');
      return verifier;
    } catch (error) {
      verifier.free();
      throw error;
    }
  }

  async pull(get, maxPages = 4) {
    if (!Number.isInteger(maxPages) || maxPages < 1 || maxPages > 64) {
      throw new Error('Invalid invitation pull limit');
    }
    const verifier = await this.load();
    try {
      let hasMore = true;
      for (let index = 0; index < maxPages && hasMore; index++) {
        const after = Number(verifier.control_cursor());
        const path = verifier.control_read_path(BigInt(after));
        const auth = verifier.sign_control_read(BigInt(after),
          crypto.getRandomValues(new Uint8Array(16)));
        const bytes = await get(path, auth);
        hasMore = verifier.accept_control_page(bytes, BigInt(after));
        const cursor = Number(verifier.control_cursor());
        if (cursor === after) break;
        const write = this.database.transaction(['invitations', 'pages'], 'readwrite');
        const done = completed(write);
        write.objectStore('pages').add({ fragment: this.fragment, after,
          bytes: Uint8Array.from(bytes) });
        write.objectStore('invitations').put({ fragment: this.fragment, cursor,
          head: hex(verifier.head_hash()), linkedIssue: verifier.linked_issue() });
        await done;
      }
      return { cursor: Number(verifier.control_cursor()), linkedIssue: verifier.linked_issue(),
        hasMore };
    } finally { verifier.free(); }
  }
}
