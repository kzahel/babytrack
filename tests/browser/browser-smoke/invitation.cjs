const { assert, chain, claimCandidateHex, claimCommittedResponseHex, input, path, setRelay, startRelay } = require('./support.cjs');

module.exports = async function ({ page, context, url, relay }) {
  let challengeRelay, invitationRelay;
  invitationRelay = await startRelay(true, 2);
  setRelay(invitationRelay);
  const invitationPull = await page.evaluate(async (fragment) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { InvitationStore } = await import('/invitation-store.js');
    const { relayGet } = await import('/relay-get.js');
    const store = await InvitationStore.open(wasm, fragment, 'babytrack-invitation-smoke');
    const pulled = await store.pull(relayGet);
    const polled = await store.pull(relayGet);
    store.close();
    return { pulled, polled };
  }, invitationRelay.fragment);
  assert.deepEqual(invitationPull, {
    pulled: { cursor: 2, linkedIssue: true, hasMore: false },
    polled: { cursor: 2, linkedIssue: true, hasMore: false },
  });
  await page.reload();
  const savedClaim = await page.evaluate(async (fragment) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { InvitationStore } = await import('/invitation-store.js');
    const store = await InvitationStore.open(wasm, fragment, 'babytrack-invitation-smoke');
    const first = await store.prepareClaim();
    const retry = await store.prepareClaim();
    store.close();
    return { same: first.candidate.length === retry.candidate.length &&
      first.candidate.every((byte, index) => byte === retry.candidate[index]),
      candidateLength: first.candidate.length };
  }, invitationRelay.fragment);
  assert.equal(savedClaim.same, true);
  assert.ok(savedClaim.candidateLength > 0);
  await page.reload();
  const lostClaimResponse = await page.evaluate(async (fragment) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { InvitationStore } = await import('/invitation-store.js');
    const { relayPostControl } = await import('/relay-post.js');
    const store = await InvitationStore.open(wasm, fragment, 'babytrack-invitation-smoke');
    let lost = false;
    try {
      await store.submitClaim(async (path, candidate) => {
        await relayPostControl(path, candidate);
        throw new Error('simulated lost claim response');
      });
    } catch (error) {
      if (error.message !== 'simulated lost claim response') throw error;
      lost = true;
    }
    const pending = await store.savedClaim();
    store.close();
    return { lost, pending: !!pending && !pending.committedResponse };
  }, invitationRelay.fragment);
  assert.deepEqual(lostClaimResponse, { lost: true, pending: true });
  await page.reload();
  const committedClaim = await page.evaluate(async (fragment) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { InvitationStore } = await import('/invitation-store.js');
    const { relayPostControl } = await import('/relay-post.js');
    const store = await InvitationStore.open(wasm, fragment, 'babytrack-invitation-smoke');
    const committed = await store.submitClaim(relayPostControl);
    const retry = await store.submitClaim(relayPostControl);
    store.close();
    return { cursor: committed.cursor, sameDevice:
      committed.deviceId.every((byte, index) => byte === retry.deviceId[index]) };
  }, invitationRelay.fragment);
  assert.deepEqual(committedClaim, { cursor: 3, sameDevice: true });
  await page.reload();
  const invitationReload = await page.evaluate(async (data) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { InvitationStore } = await import('/invitation-store.js');
    const { relayGet } = await import('/relay-get.js');
    const store = await InvitationStore.open(wasm, data.fragment, 'babytrack-invitation-smoke');
    const pending = await store.pullPending(relayGet);
    const pendingRetry = await store.pullPending(relayGet);
    const verifier = await store.load();
    const result = { cursor: Number(verifier.control_cursor()),
      linkedIssue: verifier.linked_issue() };
    verifier.free();
    let wrongOriginRejected = false;
    try {
      await InvitationStore.open(wasm, data.wrongOrigin, 'babytrack-wrong-origin-smoke');
    } catch { wrongOriginRejected = true; }
    const write = store.database.transaction('pages', 'readwrite');
    const pages = write.objectStore('pages');
    const row = await new Promise((resolve, reject) => {
      const request = pages.get([data.fragment, 0]);
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
    row.bytes[0] ^= 1;
    pages.put(row);
    await new Promise((resolve, reject) => {
      write.oncomplete = resolve;
      write.onerror = () => reject(write.error);
      write.onabort = () => reject(write.error);
    });
    let tamperRejected = false;
    try { (await store.load()).free(); } catch { tamperRejected = true; }
    store.close();
    return { ...result, pending, pendingRetry, wrongOriginRejected, tamperRejected };
  }, { fragment: invitationRelay.fragment, wrongOrigin: chain.bootstrap.fragment });
  assert.deepEqual(invitationReload, { cursor: 3, linkedIssue: true,
    pending: { cursor: 3, hasMore: false },
    pendingRetry: { cursor: 3, hasMore: false },
    wrongOriginRejected: true, tamperRejected: true });
  challengeRelay = await startRelay(true, 4);
  setRelay(challengeRelay);
  const preparedProof = await page.evaluate(async (data) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { InvitationStore } = await import('/invitation-store.js');
    const { relayGet } = await import('/relay-get.js');
    const bytes = (value) => Uint8Array.from(value.match(/../g),
      (pair) => parseInt(pair, 16));
    const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
    const store = await InvitationStore.open(wasm, data.fragment, 'babytrack-proof-smoke');
    const verifier = new wasm.WasmInvitation(data.fragment);
    verifier.accept_control_page(bytes(data.issuePrefixPageHex), 0n);
    verifier.accept_control_response(bytes(data.claimCommittedResponseHex),
      bytes(data.claimCandidateHex));
    const write = store.database.transaction(['invitations', 'pages', 'claims'], 'readwrite');
    const done = new Promise((resolve, reject) => {
      write.oncomplete = resolve;
      write.onerror = () => reject(write.error);
      write.onabort = () => reject(write.error);
    });
    write.objectStore('pages').add({ fragment: data.fragment, after: 0,
      bytes: bytes(data.issuePrefixPageHex) });
    write.objectStore('claims').add({ fragment: data.fragment, priorCursor: 2,
      candidate: bytes(data.claimCandidateHex),
      committedResponse: bytes(data.claimCommittedResponseHex),
      deviceId: bytes(data.recipientDeviceHex),
      signingSeed: bytes(data.recipientSeedHex),
      agreementPrivate: bytes(data.recipientAgreementHex) });
    write.objectStore('invitations').put({ fragment: data.fragment, cursor: 3,
      head: hex(verifier.head_hash()), linkedIssue: true });
    await done;
    verifier.free();
    const pulled = await store.pullPending(relayGet);
    const proof = await store.prepareProof(relayGet);
    const repeated = await store.prepareProof(relayGet);
    store.close();
    return { pulled, same: hex(proof.candidate) === hex(repeated.candidate),
      candidateLength: proof.candidate.length };
  }, { ...input, fragment: challengeRelay.fragment });
  assert.deepEqual(preparedProof.pulled, { cursor: 4, hasMore: false });
  assert.equal(preparedProof.same, true);
  assert.ok(preparedProof.candidateLength > 0);
  await page.reload();
  const lostProof = await page.evaluate(async (fragment) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { InvitationStore } = await import('/invitation-store.js');
    const { relayPostControl } = await import('/relay-post.js');
    const store = await InvitationStore.open(wasm, fragment, 'babytrack-proof-smoke');
    let lost = false;
    try {
      await store.submitProof(async (path, candidate) => {
        await relayPostControl(path, candidate);
        throw new Error('simulated lost proof response');
      });
    } catch (error) {
      if (error.message !== 'simulated lost proof response') throw error;
      lost = true;
    }
    const pending = await store.savedProof();
    store.close();
    return { lost, pending: !!pending && !pending.committedResponse };
  }, challengeRelay.fragment);
  assert.deepEqual(lostProof, { lost: true, pending: true });
  await page.reload();
  const confirmedProof = await page.evaluate(async (fragment) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { InvitationStore } = await import('/invitation-store.js');
    const { PublicStore } = await import('/public-store.js');
    const { relayPostControl } = await import('/relay-post.js');
    const { relayGet } = await import('/relay-get.js');
    const store = await InvitationStore.open(wasm, fragment, 'babytrack-proof-smoke');
    const cursor = await store.submitProof(relayPostControl);
    const polled = await store.pullPending(relayGet);
    const verifier = await store.load();
    const replayed = Number(verifier.control_cursor());
    verifier.free();
    const publicStore = await PublicStore.open(wasm, 'babytrack-no-grant-smoke');
    let earlyActivationRejected = false;
    try { await store.activateFirstEpoch(publicStore, relayGet); }
    catch { earlyActivationRejected = true; }
    publicStore.close();
    store.close();
    return { cursor, polled, replayed, earlyActivationRejected };
  }, challengeRelay.fragment);
  assert.deepEqual(confirmedProof,
    { cursor: 5, polled: { cursor: 5, hasMore: false }, replayed: 5,
      earlyActivationRejected: true });
};
