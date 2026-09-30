const { assert, holderBin, holderStep, http, path, raceCandidate, setRelay, spawnSync, startRelay } = require('./support.cjs');

module.exports = async function ({ page, context, url, relay }) {
  let casRelay;
  casRelay = await startRelay(false, 6, true);
  setRelay(casRelay);
  const casFragment = casRelay.fragment;
  const casBefore = await page.evaluate(async (fragment) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { InvitationStore } = await import('/invitation-store.js');
    const { relayGet } = await import('/relay-get.js');
    const store = await InvitationStore.open(wasm, fragment, 'babytrack-cas-smoke');
    await store.pull(relayGet);
    const claim = await store.prepareClaim();
    store.close();
    return { device: Array.from(claim.deviceId), nonce: Array.from(claim.enrollmentNonce),
      candidate: Array.from(claim.candidate) };
  }, casFragment);
  const unrelated = spawnSync(path.resolve(holderBin), ['later_issue',
    path.join(casRelay.temporary, 'manager.db'), `http://127.0.0.1:${casRelay.port}`,
    url.slice(0, -1)],
  { encoding: 'utf8' });
  assert.equal(unrelated.status, 0, `Could not advance claim head: ${unrelated.stderr}`);
  const casRacePage = await context.newPage();
  await casRacePage.goto(url);
  const casClaim = await page.evaluate(async (fragment) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { InvitationStore } = await import('/invitation-store.js');
    const { relayGet } = await import('/relay-get.js');
    const store = await InvitationStore.open(wasm, fragment, 'babytrack-cas-smoke');
    await store.pull(relayGet);
    store.close();
    return true;
  }, casFragment);
  assert.equal(casClaim, true);
  const claimRace = await raceCandidate(page, casRacePage, casFragment, 'claim');
  const claimOutcome = await page.evaluate(async (fragment) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { InvitationStore } = await import('/invitation-store.js');
    const { relayPostControl } = await import('/relay-post.js');
    const store = await InvitationStore.open(wasm, fragment, 'babytrack-cas-smoke');
    const saved = await store.savedClaim();
    let lost = false;
    try {
      await store.submitClaim(async (path, candidate) => {
        await relayPostControl(path, candidate);
        throw new Error('simulated lost refreshed claim response');
      });
    } catch { lost = true; }
    store.close();
    return { candidate: Array.from(saved.candidate),
      archived: (saved.archived || []).map((row) => Array.from(row.candidate)),
      lost, device: Array.from(saved.deviceId), nonce: Array.from(saved.enrollmentNonce) };
  }, casFragment);
  assert.deepEqual(claimRace.left, claimRace.right);
  assert.deepEqual(claimOutcome.candidate, claimRace.left);
  assert.notDeepEqual(claimOutcome.candidate, casBefore.candidate);
  assert.equal(claimOutcome.archived.some((row) =>
    row.toString() === casBefore.candidate.toString()), true);
  assert.deepEqual({ lost: claimOutcome.lost, device: claimOutcome.device,
    nonce: claimOutcome.nonce }, { lost: true, device: casBefore.device, nonce: casBefore.nonce });
  await page.reload();
  const recoveredCasClaim = await page.evaluate(async (fragment) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { InvitationStore } = await import('/invitation-store.js');
    const { relayGet } = await import('/relay-get.js');
    const store = await InvitationStore.open(wasm, fragment, 'babytrack-cas-smoke');
    await store.pull(relayGet);
    const recovered = await store.reconcileCandidate('claim');
    store.close();
    return { committed: !!recovered.committedResponse, fromPage: !!recovered.fromPage };
  }, casFragment);
  assert.deepEqual(recoveredCasClaim, { committed: true, fromPage: true });
  holderStep(casRelay, 'later_challenge');
  const casProofBefore = await page.evaluate(async (fragment) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { InvitationStore } = await import('/invitation-store.js');
    const { relayGet } = await import('/relay-get.js');
    const store = await InvitationStore.open(wasm, fragment, 'babytrack-cas-smoke');
    await store.pullPending(relayGet);
    const proof = await store.prepareProof(relayGet);
    store.close();
    return Array.from(proof.candidate);
  }, casFragment);
  assert.ok(casProofBefore.length > 0);
  const unrelatedProof = spawnSync(path.resolve(holderBin), ['later_issue',
    path.join(casRelay.temporary, 'manager.db'), `http://127.0.0.1:${casRelay.port}`,
    url.slice(0, -1)],
  { encoding: 'utf8' });
  assert.equal(unrelatedProof.status, 0, `Could not advance proof head: ${unrelatedProof.stderr}`);
  const casProof = await page.evaluate(async (fragment) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { InvitationStore } = await import('/invitation-store.js');
    const { relayGet } = await import('/relay-get.js');
    const store = await InvitationStore.open(wasm, fragment, 'babytrack-cas-smoke');
    await store.pullPending(relayGet);
    store.close();
    return true;
  }, casFragment);
  assert.equal(casProof, true);
  const proofRace = await raceCandidate(page, casRacePage, casFragment, 'proof');
  const proofOutcome = await page.evaluate(async (fragment) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { InvitationStore } = await import('/invitation-store.js');
    const { relayPostControl } = await import('/relay-post.js');
    const store = await InvitationStore.open(wasm, fragment, 'babytrack-cas-smoke');
    const saved = await store.savedProof();
    let lost = false;
    try {
      await store.submitProof(async (path, candidate) => {
        await relayPostControl(path, candidate);
        throw new Error('simulated lost refreshed proof response');
      });
    } catch { lost = true; }
    store.close();
    return { candidate: Array.from(saved.candidate),
      archived: (saved.archived || []).map((row) => Array.from(row.candidate)), lost };
  }, casFragment);
  assert.deepEqual(proofRace.left, proofRace.right);
  assert.deepEqual(proofOutcome.candidate, proofRace.left);
  assert.notDeepEqual(proofOutcome.candidate, casProofBefore);
  assert.equal(proofOutcome.archived.some((row) =>
    row.toString() === casProofBefore.toString()), true);
  assert.equal(proofOutcome.lost, true);
  await page.reload();
  const recoveredCasProof = await page.evaluate(async (fragment) => {
    const wasm = await import('/babytrack_core_wasm.js');
    await wasm.default('/babytrack_core_wasm_bg.wasm');
    const { InvitationStore } = await import('/invitation-store.js');
    const { relayGet } = await import('/relay-get.js');
    const store = await InvitationStore.open(wasm, fragment, 'babytrack-cas-smoke');
    await store.pullPending(relayGet);
    const recovered = await store.reconcileCandidate('proof');
    store.close();
    return { committed: !!recovered.committedResponse, fromPage: !!recovered.fromPage };
  }, casFragment);
  assert.deepEqual(recoveredCasProof, { committed: true, fromPage: true });
  await casRacePage.close();
};
