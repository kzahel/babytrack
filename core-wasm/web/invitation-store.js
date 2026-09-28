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
const randomV4 = () => {
  const id = crypto.getRandomValues(new Uint8Array(16));
  id[6] = (id[6] & 0x0f) | 0x40;
  id[8] = (id[8] & 0x3f) | 0x80;
  return id;
};

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
    const request = indexedDB.open(name, 2);
    request.onupgradeneeded = () => {
      const database = request.result;
      if (!database.objectStoreNames.contains('invitations')) {
        database.createObjectStore('invitations', { keyPath: 'fragment' });
      }
      if (!database.objectStoreNames.contains('pages')) {
        database.createObjectStore('pages', { keyPath: ['fragment', 'after'] });
      }
      if (!database.objectStoreNames.contains('claims')) {
        database.createObjectStore('claims', { keyPath: 'fragment' });
      }
    };
    return new InvitationStore(await result(request), wasm, fragment);
  }

  close() { this.database.close(); }

  async load() {
    const read = this.database.transaction(['invitations', 'pages', 'claims'], 'readonly');
    const metadataRequest = read.objectStore('invitations').get(this.fragment);
    const claimRequest = read.objectStore('claims').get(this.fragment);
    const pagesRequest = read.objectStore('pages').getAll(
      IDBKeyRange.bound([this.fragment, 0], [this.fragment, Number.MAX_SAFE_INTEGER]),
    );
    const [metadata, pages, claim] = await Promise.all([
      result(metadataRequest), result(pagesRequest), result(claimRequest),
    ]);
    const verifier = new this.wasm.WasmInvitation(this.fragment);
    try {
      let claimApplied = false;
      for (const page of pages) {
        if (claim?.committedResponse && !claimApplied && page.after > claim.priorCursor) {
          verifier.accept_claim_response(claim.committedResponse, claim.candidate);
          claimApplied = true;
        }
        if (Number(verifier.control_cursor()) !== page.after) {
          throw new Error('Saved invitation page order differs');
        }
        verifier.accept_control_page(page.bytes, BigInt(page.after));
      }
      if (claim?.committedResponse && !claimApplied) {
        verifier.accept_claim_response(claim.committedResponse, claim.candidate);
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
    const claim = await this.savedClaim();
    if (claim) throw new Error('Invitation read is closed after claim preparation');
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

  async savedClaim() {
    const read = this.database.transaction('claims', 'readonly');
    return result(read.objectStore('claims').get(this.fragment));
  }

  // Persist the exact candidate and private keys before any network write.
  async prepareClaim() {
    const prior = await this.savedClaim();
    if (prior) return prior;
    const verifier = await this.load();
    try {
      const deviceId = randomV4();
      const signingSeed = crypto.getRandomValues(new Uint8Array(32));
      const agreementPrivate = crypto.getRandomValues(new Uint8Array(32));
      const enrollmentNonce = crypto.getRandomValues(new Uint8Array(32));
      const transitionId = randomV4();
      const candidate = verifier.prepare_claim(deviceId, signingSeed, agreementPrivate,
        enrollmentNonce, transitionId);
      const claim = { fragment: this.fragment, deviceId, signingSeed, agreementPrivate,
        enrollmentNonce, transitionId, candidate: Uint8Array.from(candidate),
        priorCursor: Number(verifier.control_cursor()) };
      const write = this.database.transaction('claims', 'readwrite');
      const done = completed(write);
      write.objectStore('claims').add(claim);
      await done;
      return claim;
    } finally { verifier.free(); }
  }

  async submitClaim(post) {
    const claim = await this.savedClaim();
    if (!claim) throw new Error('Claim must be saved before posting');
    const verifier = await this.load();
    try {
      if (claim.committedResponse) {
        return { cursor: Number(verifier.control_cursor()), deviceId: claim.deviceId };
      }
      const family = hex(verifier.family_id());
      const response = await post(`/v1/families/${family}/control`, claim.candidate);
      verifier.accept_claim_response(response, claim.candidate);
      const write = this.database.transaction(['claims', 'invitations'], 'readwrite');
      const done = completed(write);
      write.objectStore('claims').put({ ...claim, committedResponse: Uint8Array.from(response) });
      write.objectStore('invitations').put({ fragment: this.fragment,
        cursor: Number(verifier.control_cursor()), head: hex(verifier.head_hash()),
        linkedIssue: verifier.linked_issue() });
      await done;
      return { cursor: Number(verifier.control_cursor()), deviceId: claim.deviceId };
    } finally { verifier.free(); }
  }

  async pullPending(get, maxPages = 4) {
    const claim = await this.savedClaim();
    if (!claim?.committedResponse) throw new Error('Committed claim required for pending read');
    if (!Number.isInteger(maxPages) || maxPages < 1 || maxPages > 64) {
      throw new Error('Invalid pending pull limit');
    }
    const verifier = await this.load();
    try {
      let hasMore = true;
      for (let index = 0; index < maxPages && hasMore; index++) {
        const after = Number(verifier.control_cursor());
        const path = verifier.control_read_path(BigInt(after));
        const auth = verifier.sign_pending_control_read(BigInt(after), claim.deviceId,
          claim.signingSeed, crypto.getRandomValues(new Uint8Array(16)));
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
      return { cursor: Number(verifier.control_cursor()), hasMore };
    } finally { verifier.free(); }
  }
}
