// An invitation can read public controls before it has a device credential.
// Persist the exact pages and replay them through the Rust verifier on load.
const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
const sameBytes = (left, right) => left.length === right.length &&
  left.every((byte, index) => byte === right[index]);
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
    const request = indexedDB.open(name, 4);
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
      if (!database.objectStoreNames.contains('proofs')) {
        database.createObjectStore('proofs', { keyPath: 'fragment' });
      }
      if (!database.objectStoreNames.contains('drafts')) {
        database.createObjectStore('drafts', { keyPath: 'fragment' });
      }
    };
    const store = new InvitationStore(await result(request), wasm, fragment);
    const write = store.database.transaction('drafts', 'readwrite');
    const done = completed(write);
    const drafts = write.objectStore('drafts');
    const prior = await result(drafts.get(fragment));
    drafts.put({ ...prior, fragment, lastTouched: Date.now() });
    await done;
    return store;
  }

  close() { this.database.close(); }

  async isApproved() {
    const read = this.database.transaction('drafts', 'readonly');
    const draft = await result(read.objectStore('drafts').get(this.fragment));
    return !!draft?.approved;
  }

  async hasSavedTerminal() {
    const read = this.database.transaction('drafts', 'readonly');
    const draft = await result(read.objectStore('drafts').get(this.fragment));
    if (!draft?.terminalResponse) return false;
    const verifier = await this.load();
    try { return verifier.verify_status_reason(draft.terminalResponse) >= 2; }
    finally { verifier.free(); }
  }

  async approve() {
    const write = this.database.transaction('drafts', 'readwrite');
    const done = completed(write);
    const drafts = write.objectStore('drafts');
    const prior = await result(drafts.get(this.fragment));
    drafts.put({ ...prior, fragment: this.fragment, approved: true, lastTouched: Date.now() });
    await done;
  }

  static async pendingFragments(name = 'babytrack-invitations') {
    const request = indexedDB.open(name);
    const database = await result(request);
    try {
      if (!database.objectStoreNames.contains('drafts')) return [];
      const read = database.transaction('drafts', 'readonly');
      return (await result(read.objectStore('drafts').getAll()))
        .sort((left, right) => (right.lastTouched || 0) - (left.lastTouched || 0))
        .map((row) => row.fragment);
    } finally { database.close(); }
  }

  async forget() {
    const write = this.database.transaction(['drafts', 'invitations', 'pages', 'claims', 'proofs'], 'readwrite');
    const done = completed(write);
    write.objectStore('drafts').delete(this.fragment);
    write.objectStore('invitations').delete(this.fragment);
    write.objectStore('claims').delete(this.fragment);
    write.objectStore('proofs').delete(this.fragment);
    const pages = write.objectStore('pages');
    const keys = await result(pages.getAllKeys(IDBKeyRange.bound(
      [this.fragment, 0], [this.fragment, Number.MAX_SAFE_INTEGER],
    )));
    for (const key of keys) pages.delete(key);
    await done;
  }

  async load() {
    const read = this.database.transaction(['invitations', 'pages', 'claims', 'proofs'], 'readonly');
    const metadataRequest = read.objectStore('invitations').get(this.fragment);
    const claimRequest = read.objectStore('claims').get(this.fragment);
    const proofRequest = read.objectStore('proofs').get(this.fragment);
    const pagesRequest = read.objectStore('pages').getAll(
      IDBKeyRange.bound([this.fragment, 0], [this.fragment, Number.MAX_SAFE_INTEGER]),
    );
    const [metadata, pages, claim, proof] = await Promise.all([
      result(metadataRequest), result(pagesRequest), result(claimRequest), result(proofRequest),
    ]);
    const verifier = new this.wasm.WasmInvitation(this.fragment);
    try {
      let claimApplied = false;
      let proofApplied = false;
      for (const page of pages) {
        if (claim?.committedResponse && !claim.fromPage && !claimApplied && page.after > claim.priorCursor) {
          verifier.accept_control_response(claim.committedResponse, claim.candidate);
          claimApplied = true;
        }
        if (proof?.committedResponse && !proof.fromPage && !proofApplied && page.after > proof.priorCursor) {
          verifier.accept_control_response(proof.committedResponse, proof.candidate);
          proofApplied = true;
        }
        if (Number(verifier.control_cursor()) !== page.after) {
          throw new Error('Saved invitation page order differs');
        }
        verifier.accept_control_page(page.bytes, BigInt(page.after));
      }
      if (claim?.committedResponse && !claim.fromPage && !claimApplied) {
        verifier.accept_control_response(claim.committedResponse, claim.candidate);
      }
      if (proof?.committedResponse && !proof.fromPage && !proofApplied) {
        verifier.accept_control_response(proof.committedResponse, proof.candidate);
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
    if (claim?.committedResponse) throw new Error('Invitation read is closed after claim commit');
    const verifier = await this.load();
    try {
      let hasMore = true;
      for (let index = 0; index < maxPages && hasMore; index++) {
        const after = Number(verifier.control_cursor());
        const path = verifier.control_read_path(BigInt(after));
        const auth = verifier.sign_control_read(BigInt(after),
          crypto.getRandomValues(new Uint8Array(16)));
        const response = get(path, auth);
        const bytes = claim ? await response.catch(async () => get(path,
          verifier.sign_pending_control_read(BigInt(after), claim.deviceId,
            claim.signingSeed, crypto.getRandomValues(new Uint8Array(16))))) : await response;
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

  async status(get) {
    const verifier = await this.load();
    try {
      const read = this.database.transaction('drafts', 'readonly');
      const saved = await result(read.objectStore('drafts').get(this.fragment));
      if (saved?.terminalResponse) return verifier.verify_status_reason(saved.terminalResponse);
      const bytes = await get(verifier.status_read_path(),
        verifier.sign_status_read(crypto.getRandomValues(new Uint8Array(16))));
      const reason = verifier.verify_status_reason(bytes);
      if (reason >= 3 || (reason === 2 && !await this.savedClaim())) {
        const write = this.database.transaction('drafts', 'readwrite');
        const done = completed(write);
        const drafts = write.objectStore('drafts');
        const current = await result(drafts.get(this.fragment));
        drafts.put({ ...current, fragment: this.fragment,
          terminalResponse: Uint8Array.from(bytes) });
        await done;
      }
      return reason;
    } finally { verifier.free(); }
  }

  async reconcileCandidate(kind) {
    const saved = kind === 'claim' ? await this.savedClaim() : await this.savedProof();
    if (!saved || saved.committedResponse) return saved;
    const verifier = await this.load();
    try {
      for (const row of [saved, ...(saved.archived || [])]) {
        const result = verifier.candidate_result_in_history(row.candidate);
        if (!result.length) continue;
        const next = { ...saved, candidate: row.candidate,
          transitionId: row.transitionId, committedResponse: result, fromPage: true };
        const write = this.database.transaction(kind === 'claim' ? 'claims' : 'proofs', 'readwrite');
        const done = completed(write);
        write.objectStore(kind === 'claim' ? 'claims' : 'proofs').put(next);
        await done;
        return next;
      }
      return saved;
    } finally { verifier.free(); }
  }

  async refreshCandidate(kind, beforeWrite = null) {
    const saved = kind === 'claim' ? await this.savedClaim() : await this.savedProof();
    if (!saved || saved.committedResponse) return saved;
    const verifier = await this.load();
    try {
      if (verifier.candidate_uses_current_head(saved.candidate)) return saved;
      const transitionId = randomV4();
      const candidate = kind === 'claim' ? verifier.prepare_claim(saved.deviceId,
        saved.signingSeed, saved.agreementPrivate, saved.enrollmentNonce, transitionId) :
        verifier.prepare_proof((await this.savedClaim()).deviceId,
          (await this.savedClaim()).signingSeed, (await this.savedClaim()).agreementPrivate,
          saved.objectResponse, transitionId);
      if (beforeWrite) await beforeWrite();
      const name = kind === 'claim' ? 'claims' : 'proofs';
      const write = this.database.transaction(name, 'readwrite');
      const done = completed(write);
      const rows = write.objectStore(name);
      const current = await result(rows.get(this.fragment));
      if (!current) {
        write.abort();
        await done.catch(() => {});
        throw new Error('Saved enrollment candidate disappeared during refresh');
      }
      if (current.committedResponse || !sameBytes(current.candidate, saved.candidate)) {
        await done;
        return current;
      }
      const archived = [...(current.archived || []),
        { candidate: current.candidate, transitionId: current.transitionId }];
      const seen = new Set();
      const next = { ...current, transitionId, candidate: Uint8Array.from(candidate),
        priorCursor: Number(verifier.control_cursor()),
        archived: archived.filter((row) => {
          const key = hex(row.candidate);
          if (seen.has(key)) return false;
          seen.add(key);
          return true;
        }) };
      rows.put(next);
      await done;
      return next;
    } finally { verifier.free(); }
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
      verifier.accept_control_response(response, claim.candidate);
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

  async savedProof() {
    const read = this.database.transaction('proofs', 'readonly');
    return result(read.objectStore('proofs').get(this.fragment));
  }

  async prepareProof(get) {
    const prior = await this.savedProof();
    if (prior) return prior;
    const claim = await this.savedClaim();
    if (!claim?.committedResponse) throw new Error('Committed claim required for proof');
    const verifier = await this.load();
    try {
      const objectId = verifier.challenge_hpke_object_id();
      const objectHex = hex(objectId);
      const family = hex(verifier.family_id());
      const path = `/v1/families/${family}/objects/${objectHex}`;
      const auth = verifier.sign_challenge_object_read(objectId, claim.deviceId,
        claim.signingSeed, crypto.getRandomValues(new Uint8Array(16)));
      const response = await get(path, auth);
      const transitionId = randomV4();
      const candidate = verifier.prepare_proof(claim.deviceId, claim.signingSeed,
        claim.agreementPrivate, response, transitionId);
      const proof = { fragment: this.fragment, candidate: Uint8Array.from(candidate),
        transitionId, objectResponse: Uint8Array.from(response),
        priorCursor: Number(verifier.control_cursor()) };
      const write = this.database.transaction('proofs', 'readwrite');
      const done = completed(write);
      write.objectStore('proofs').add(proof);
      await done;
      return proof;
    } finally { verifier.free(); }
  }

  async submitProof(post) {
    const proof = await this.savedProof();
    if (!proof) throw new Error('Proof must be saved before posting');
    const verifier = await this.load();
    try {
      if (proof.committedResponse) return Number(verifier.control_cursor());
      const family = hex(verifier.family_id());
      const response = await post(`/v1/families/${family}/control`, proof.candidate);
      verifier.accept_control_response(response, proof.candidate);
      const write = this.database.transaction(['proofs', 'invitations'], 'readwrite');
      const done = completed(write);
      write.objectStore('proofs').put({ ...proof, committedResponse: Uint8Array.from(response) });
      write.objectStore('invitations').put({ fragment: this.fragment,
        cursor: Number(verifier.control_cursor()), head: hex(verifier.head_hash()),
        linkedIssue: verifier.linked_issue() });
      await done;
      return Number(verifier.control_cursor());
    } finally { verifier.free(); }
  }

  // Once the sparse chain proves this device's grant, rebuild the full
  // public log before opening any data key. The existing PublicStore owns
  // contiguous log, object, credential, and encrypted record readiness.
  async activateAdmitted(publicStore, get) {
    const claim = await this.savedClaim();
    if (!claim?.committedResponse) throw new Error('Committed claim required for admission');
    const verifier = await this.load();
    let family;
    try {
      if (!verifier.has_admission(claim.deviceId)) {
        throw new Error('No verified admission grant for this device');
      }
      family = hex(verifier.family_id());
      const genesis = verifier.genesis_bytes();
      const relayPublicKey = verifier.relay_public_key();
      const read = publicStore.database.transaction('families', 'readonly');
      const metadata = await result(read.objectStore('families').get(family));
      if (metadata) {
        if (!sameBytes(metadata.genesis, genesis) ||
            !sameBytes(metadata.relayPublicKey, relayPublicKey)) {
          throw new Error('Full-history Family pin differs from invitation');
        }
      } else {
        await publicStore.begin(genesis, relayPublicKey);
      }
      const progress = await publicStore.pull(family, claim.deviceId, claim.signingSeed, get, 64,
        (path) => verifier.sign_family_read(path, claim.deviceId, claim.signingSeed,
          crypto.getRandomValues(new Uint8Array(16))));
      if (!progress.noMoreVisible) return { family, loadingHistory: true, cursor: progress.cursor };
    } finally { verifier.free(); }
    await publicStore.hydrateGenesis(family, claim.deviceId, claim.signingSeed, get);
    await publicStore.hydrateControlObjects(family, claim.deviceId, claim.signingSeed, get);
    await publicStore.saveAdmittedCredential(family, claim.deviceId,
      claim.signingSeed, claim.agreementPrivate);
    const ready = await publicStore.loadInitialReadySaved(family);
    try {
      return { family, cursor: Number(ready.last_cursor()) };
    } finally { ready.free(); }
  }

  async activateFirstEpoch(publicStore, get) {
    return this.activateAdmitted(publicStore, get);
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
