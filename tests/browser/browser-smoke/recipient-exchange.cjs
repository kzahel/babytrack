const { assert, challengePageHex, claimCandidateHex, claimCommittedResponseHex, grantIdHex, input, path, proofCandidateHex, proofCommittedResponseHex, proofTransitionIdHex, setRelay, startRelay } = require('./support.cjs');

module.exports = async function ({ page, context, url, relay }) {
  let recipientRelay;
  recipientRelay = await startRelay(true);
  setRelay(recipientRelay);
  const browserProof = await page.evaluate(async (data) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { relayGet } = await import('/relay-get.js');
    const bytes = (value) => Uint8Array.from(value.match(/../g),
      (pair) => parseInt(pair, 16));
    const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
    const invitation = new wasm.WasmInvitation(data.fragment);
    invitation.accept_control_page(bytes(data.challengePrefixPageHex), 0n);
    const objectId = invitation.challenge_hpke_object_id();
    const path = `/v1/families/${data.familyHex}/objects/${hex(objectId)}`;
    const auth = invitation.sign_challenge_object_read(objectId,
      bytes(data.recipientDeviceHex), bytes(data.recipientSeedHex),
      crypto.getRandomValues(new Uint8Array(16)));
    const object = await relayGet(path, auth);
    const candidate = invitation.prepare_proof(bytes(data.recipientDeviceHex),
      bytes(data.recipientSeedHex), bytes(data.recipientAgreementHex), object,
      bytes(data.proofTransitionIdHex));
    let wrongKeyRejected = false;
    try {
      invitation.prepare_proof(bytes(data.recipientDeviceHex),
        bytes(data.recipientSeedHex), new Uint8Array(32), object,
        bytes(data.proofTransitionIdHex));
    } catch { wrongKeyRejected = true; }
    const changed = Uint8Array.from(object);
    changed[changed.length - 1] ^= 1;
    let wrongObjectRejected = false;
    try {
      invitation.prepare_proof(bytes(data.recipientDeviceHex),
        bytes(data.recipientSeedHex), bytes(data.recipientAgreementHex), changed,
        bytes(data.proofTransitionIdHex));
    } catch { wrongObjectRejected = true; }
    invitation.free();
    return { objectId: hex(objectId), candidate: hex(candidate),
      wrongKeyRejected, wrongObjectRejected };
  }, { ...input, fragment: recipientRelay.fragment });
  assert.deepEqual(browserProof, { objectId: input.challengeObjectIdHex,
    candidate: input.proofCandidateHex, wrongKeyRejected: true,
    wrongObjectRejected: true });
  const browserActivation = await page.evaluate(async (data) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { InvitationStore } = await import('/invitation-store.js');
    const { PublicStore } = await import('/public-store.js');
    const { relayGet } = await import('/relay-get.js');
    const bytes = (value) => Uint8Array.from(value.match(/../g),
      (pair) => parseInt(pair, 16));
    const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
    const store = await InvitationStore.open(wasm, data.fragment, 'babytrack-activation-smoke');
    const verifier = new wasm.WasmInvitation(data.fragment);
    verifier.accept_control_page(bytes(data.issuePrefixPageHex), 0n);
    verifier.accept_control_response(bytes(data.claimCommittedResponseHex),
      bytes(data.claimCandidateHex));
    verifier.accept_control_page(bytes(data.challengePageHex), 3n);
    verifier.accept_control_response(bytes(data.proofCommittedResponseHex),
      bytes(data.proofCandidateHex));
    const write = store.database.transaction(['invitations', 'pages', 'claims', 'proofs'], 'readwrite');
    const done = new Promise((resolve, reject) => {
      write.oncomplete = resolve;
      write.onerror = () => reject(write.error);
      write.onabort = () => reject(write.error);
    });
    write.objectStore('pages').add({ fragment: data.fragment, after: 0,
      bytes: bytes(data.issuePrefixPageHex) });
    write.objectStore('pages').add({ fragment: data.fragment, after: 3,
      bytes: bytes(data.challengePageHex) });
    write.objectStore('claims').add({ fragment: data.fragment, priorCursor: 2,
      candidate: bytes(data.claimCandidateHex),
      committedResponse: bytes(data.claimCommittedResponseHex),
      deviceId: bytes(data.recipientDeviceHex), signingSeed: bytes(data.recipientSeedHex),
      agreementPrivate: bytes(data.recipientAgreementHex) });
    write.objectStore('proofs').add({ fragment: data.fragment, priorCursor: 4,
      candidate: bytes(data.proofCandidateHex),
      committedResponse: bytes(data.proofCommittedResponseHex) });
    write.objectStore('invitations').put({ fragment: data.fragment, cursor: 5,
      head: hex(verifier.head_hash()), linkedIssue: true });
    await done;
    verifier.free();
    const pending = await store.pullPending(relayGet);
    const publicStore = await PublicStore.open(wasm, 'babytrack-activated-browser-smoke');
    const activated = await store.activateFirstEpoch(publicStore, relayGet);
    const credential = await publicStore.initialCredential(activated.family);
    publicStore.close();
    store.close();
    return { pending, activated, device: hex(credential.deviceId),
      key: hex(credential.epochKey) };
  }, { ...input, fragment: recipientRelay.fragment });
  assert.deepEqual(browserActivation, {
    pending: { cursor: 6, hasMore: false },
    activated: { family: input.familyHex, cursor: 6 },
    device: input.recipientDeviceHex, key: input.controlEpochKeyHex,
  });
  await page.reload();
  const activationReload = await page.evaluate(async (fragment) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { InvitationStore } = await import('/invitation-store.js');
    const { PublicStore } = await import('/public-store.js');
    const { relayGet } = await import('/relay-get.js');
    const store = await InvitationStore.open(wasm, fragment, 'babytrack-activation-smoke');
    const publicStore = await PublicStore.open(wasm, 'babytrack-activated-browser-smoke');
    const result = await store.activateFirstEpoch(publicStore, relayGet);
    publicStore.close();
    store.close();
    return result;
  }, recipientRelay.fragment);
  assert.deepEqual(activationReload, { family: input.familyHex, cursor: 6 });
  const admittedBrowser = await page.evaluate(async (data) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { PublicStore } = await import('/public-store.js');
    const { relayGet } = await import('/relay-get.js');
    const bytes = (value) => Uint8Array.from(value.match(/../g), (pair) => parseInt(pair, 16));
    const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
    const store = await PublicStore.open(wasm, 'babytrack-admitted-browser-smoke');
    const family = await store.begin(bytes(data.controlGenesisHex), bytes(data.relayPublicHex));
    for (const control of data.admissionControlsHex) await store.append(family, 'control', bytes(control));
    let shortcutRejected = false;
    try {
      await store.saveInitialCredential(family, bytes(data.recipientDeviceHex),
        bytes(data.recipientSeedHex), bytes(data.controlEpochKeyHex),
        bytes(data.recipientAgreementHex));
    } catch { shortcutRejected = true; }
    let missingGrantRejected = false;
    try {
      await store.saveAdmittedCredential(family, bytes(data.recipientDeviceHex),
        bytes(data.recipientSeedHex), bytes(data.recipientAgreementHex));
    } catch { missingGrantRejected = true; }
    const fetchedId = await store.hydrateInitialGrant(family, bytes(data.recipientDeviceHex),
      bytes(data.recipientSeedHex), relayGet);
    let wrongKeyRejected = false;
    try {
      await store.saveAdmittedCredential(family, bytes(data.recipientDeviceHex),
        bytes(data.recipientSeedHex), new Uint8Array(32));
    } catch { wrongKeyRejected = true; }
    await store.saveAdmittedCredential(family, bytes(data.recipientDeviceHex),
      bytes(data.recipientSeedHex), bytes(data.recipientAgreementHex));
    await store.hydrateControlObjectsSaved(family, relayGet);
    const credential = await store.initialCredential(family);
    const promotion = store.database.transaction('objects', 'readwrite');
    const committed = new Promise((resolve, reject) => {
      promotion.oncomplete = resolve;
      promotion.onabort = () => reject(promotion.error);
      promotion.onerror = () => reject(promotion.error);
    });
    promotion.objectStore('objects').put({ family,
      objectId: data.controlPromotionIdHex, bytes: bytes(data.controlPromotionHex) });
    await committed;
    const ready = await store.loadInitialReadySaved(family);
    const cursor = Number(ready.last_cursor());
    ready.free();
    const staged = await store.stageInitial(family, bytes(data.recipientOperationHex));
    store.close();
    return { shortcutRejected, missingGrantRejected, wrongKeyRejected, fetchedId,
      credentialDevice: hex(credential.deviceId), credentialKey: hex(credential.epochKey),
      agreementSaved: hex(credential.agreementPrivate) === data.recipientAgreementHex,
      cursor, staged: staged.envelope.length > 0 };
  }, input);
  assert.deepEqual(admittedBrowser, {
    shortcutRejected: true, missingGrantRejected: true, wrongKeyRejected: true,
    fetchedId: input.grantIdHex,
    credentialDevice: input.recipientDeviceHex, credentialKey: input.controlEpochKeyHex,
    agreementSaved: true, cursor: 6, staged: true,
  });
  await page.reload();
  const admittedReload = await page.evaluate(async (data) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { PublicStore } = await import('/public-store.js');
    const store = await PublicStore.open(wasm, 'babytrack-admitted-browser-smoke');
    const pending = await store.pendingInitial(data.familyHex);
    const ready = await store.loadInitialReadySaved(data.familyHex);
    const result = { pending: pending?.envelope.length > 0,
      cursor: Number(ready.last_cursor()), agreementSaved:
        (await store.initialCredential(data.familyHex)).agreementPrivate.length === 32 };
    ready.free();
    store.close();
    return result;
  }, input);
  assert.deepEqual(admittedReload, { pending: true, cursor: 6, agreementSaved: true });
  const admittedUpload = await page.evaluate(async (data) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { PublicStore } = await import('/public-store.js');
    const { relayGet } = await import('/relay-get.js');
    const { relayPost } = await import('/relay-post.js');
    const bytes = (value) => Uint8Array.from(value.match(/../g), (pair) => parseInt(pair, 16));
    const store = await PublicStore.open(wasm, 'babytrack-admitted-browser-smoke');
    const progress = await store.uploadInitial(data.familyHex, relayPost, relayGet);
    const ready = await store.loadInitialReadySaved(data.familyHex);
    const result = { ...progress, pending: !!(await store.pendingInitial(data.familyHex)),
      childType: ready.record_type(bytes('0183f9d0000070008000000000000001')) };
    ready.free();
    store.close();
    return result;
  }, input);
  assert.deepEqual(admittedUpload,
    { cursor: 7, noMoreVisible: true, pending: false, childType: 'child' });
  const recipientSeenByManager = await page.evaluate(async (data) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { PublicStore } = await import('/public-store.js');
    const { relayGet } = await import('/relay-get.js');
    const bytes = (value) => Uint8Array.from(value.match(/../g),
      (pair) => parseInt(pair, 16));
    const hex = (value) => Array.from(value,
      (byte) => byte.toString(16).padStart(2, '0')).join('');
    const store = await PublicStore.open(wasm, 'babytrack-manager-reads-recipient-smoke');
    const family = await store.begin(bytes(data.controlGenesisHex), bytes(data.relayPublicHex));
    for (const control of data.admissionControlsHex) {
      await store.append(family, 'control', bytes(control));
    }
    const objectWrite = store.database.transaction('objects', 'readwrite');
    const objectCommitted = new Promise((resolve, reject) => {
      objectWrite.oncomplete = resolve;
      objectWrite.onabort = () => reject(objectWrite.error);
      objectWrite.onerror = () => reject(objectWrite.error);
    });
    objectWrite.objectStore('objects').put({ family,
      objectId: data.controlPromotionIdHex, bytes: bytes(data.controlPromotionHex) });
    await objectCommitted;
    await store.saveInitialCredential(family, bytes(data.managerDeviceHex),
      bytes(data.managerSeedHex), bytes(data.controlEpochKeyHex),
      bytes(data.managerAgreementHex));
    const progress = await store.pullSaved(family, relayGet);
    await store.hydrateControlObjectsSaved(family, relayGet);
    const ready = await store.loadInitialReadySaved(family);
    const child = bytes('0183f9d0000070008000000000000001');
    const result = { ...progress, childType: ready.record_type(child),
      childNameCbor: hex(ready.field_cbor(child, 1n)) };
    ready.free();
    store.close();
    return result;
  }, input);
  assert.deepEqual(recipientSeenByManager,
    { cursor: 7, noMoreVisible: true, childType: 'child', childNameCbor: '6442616279' });
};
