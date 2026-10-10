import { InvitationStore } from '../../../core-wasm/web/invitation-store.js';
import { LocalStore } from '../../../core-wasm/web/local-store.js';
import { PublicStore } from '../../../core-wasm/web/public-store.js';
import { relayGet } from '../../../core-wasm/web/relay-get.js';
import { relayPost, relayPostControl } from '../../../core-wasm/web/relay-post.js';

const invitationDatabase = 'babytrack-preview-invitations-v1';
const publicDatabase = 'babytrack-preview-public-v1';
const localDatabase = 'babytrack-preview-local-v1';
const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
const bytes = (value) => Uint8Array.from(value.match(/../g) || [], (pair) => parseInt(pair, 16));

export async function sharedFamilies(wasm) {
  const store = await PublicStore.open(wasm, publicDatabase);
  try {
    const readyRows = [];
    for (const row of await store.families()) {
      try {
        const ready = await store.loadInitialReadySaved(row.family);
        ready.free();
        readyRows.push(row);
      } catch { /* A saved public prefix without a verified key is still joining. */ }
    }
    return readyRows;
  }
  finally { store.close(); }
}

// Read the verified shared state, including pending browser edits.
export async function readShared(wasm, family, read) {
  const store = await PublicStore.open(wasm, publicDatabase);
  try {
    const ready = await store.loadInitialReadySaved(family);
    try { return read(ready); }
    finally { ready.free(); }
  } finally { store.close(); }
}

export async function exportSharedReadable(wasm, family) {
  const store = await PublicStore.open(wasm, publicDatabase);
  try {
    const removal = await store.removedStatus(family);
    const ready = await store.loadInitialReadySaved(family);
    const history = await store.historyStatus(family);
    try { return ready.readable_file(BigInt(Date.now()),
      !!removal?.knownGap || history.knownIncomplete); }
    finally { ready.free(); }
  } finally { store.close(); }
}

export async function syncShared(wasm, family) {
  const store = await PublicStore.open(wasm, publicDatabase);
  try {
    const removal = await store.probeRemoval(family, relayGet);
    if (removal) return { removed: true, privateCopy: await ensureRemovalCopy(wasm, store, family, removal) };
    let progress = await store.pullSaved(family, relayGet, 16);
    await store.hydrateControlObjectsSaved(family, relayGet);
    if (await store.pendingInitial(family) || (await store.queuedInitial(family)).length) {
      progress = await store.uploadInitial(family, relayPost, relayGet);
    }
    const ready = await store.loadInitialReadySaved(family);
    ready.free();
    return progress;
  } finally { store.close(); }
}

export async function writeShared(wasm, family, action, values, localAction) {
  const store = await PublicStore.open(wasm, publicDatabase);
  try {
    const removed = await store.removedStatus(family);
    if (removed) return { redirectFamily: await ensureRemovalCopy(wasm, store, family, removed,
      true, localAction) };
    const credential = await store.initialCredential(family);
    const ready = await store.loadInitialReadySaved(family);
    let operation;
    try {
      const clock = credential.clock || { wallMs: -1, counter: 0 };
      const prefix = [credential.deviceId, BigInt(clock.wallMs), clock.counter];
      const now = BigInt(Date.now());
      if (action !== 'action') throw new Error('Unknown shared action');
      operation = ready.action_operation(...prefix, values.json, now);
      try { await store.queueInitial(family, operation, clock); }
      catch (error) {
        const removal = await store.removedStatus(family);
        if (removal) return { redirectFamily: await ensureRemovalCopy(wasm, store, family, removal,
          true, localAction) };
        throw error;
      }
    } finally { ready.free(); }
    return { pending: true };
  } finally { store.close(); }
}

async function ensureRemovalCopy(wasm, store, family, removal, force = false, localAction = null) {
  const local = await LocalStore.open(wasm, localDatabase);
  try {
    const existing = await local.removalCopy(family, removal.transitionId);
    if (existing && !localAction) return existing.family;
    if (existing) {
      return local.createRemovalCopyWithAction(family, removal.transitionId,
        existing.family, '', [], localAction.deliveryId, localAction.prepare);
    }
    if (!force && !await store.pendingInitial(family) && !(await store.queuedInitial(family)).length) {
      return null;
    }
    const ready = await store.loadInitialReadySaved(family);
    let readable;
    const history = await store.historyStatus(family);
    try { readable = ready.readable_file(BigInt(Date.now()),
      removal.knownGap || history.knownIncomplete); }
    finally { ready.free(); }
    const ids = wasm.new_local_ids();
    const newFamily = hex(ids.slice(0, 16));
    const newDevice = hex(ids.slice(16));
    const restore = new wasm.WasmReadableRestore(
      readable, ids.slice(0, 16), ids.slice(16), BigInt(Date.now()));
    try {
      const operations = Array.from({ length: restore.count() }, (_, index) => restore.operation(index));
      if (localAction) {
        return local.createRemovalCopyWithAction(family, removal.transitionId,
          newFamily, newDevice, operations, localAction.deliveryId, localAction.prepare);
      }
      return local.createRemovalCopy(family, removal.transitionId,
        newFamily, newDevice, operations);
    } finally { restore.free(); }
  } finally { local.close(); }
}

export async function copyRemoved(wasm, family) {
  const store = await PublicStore.open(wasm, publicDatabase);
  try {
    const removal = await store.removedStatus(family);
    if (!removal) throw new Error('Verified removal is required before copying');
    return await ensureRemovalCopy(wasm, store, family, removal, true);
  } finally { store.close(); }
}

export async function pendingJoins() {
  return InvitationStore.pendingFragments(invitationDatabase);
}

export function invitationFragment(value) {
  const fragment = value.trim().startsWith('#') ? value.trim() :
    new URL(value.trim(), location.origin).hash;
  if (!fragment.startsWith('#bt-invite=v1.') || fragment.length > 2048) {
    throw new Error('Enter a Babytrack invitation link');
  }
  return fragment;
}

export async function rememberJoin(wasm, value) {
  const fragment = invitationFragment(value);
  const store = await InvitationStore.open(wasm, fragment, invitationDatabase);
  store.close();
  return fragment;
}

export async function approveJoin(wasm, fragment) {
  const store = await InvitationStore.open(wasm, fragment, invitationDatabase);
  try { await store.approve(); } finally { store.close(); }
}

export async function dismissJoin(wasm, fragment) {
  const store = await InvitationStore.open(wasm, fragment, invitationDatabase);
  try {
    if (await store.isApproved() && !await store.hasSavedTerminal()) {
      throw new Error('Joining already started');
    }
    await store.forget();
  } finally { store.close(); }
}

// Each call advances as far as the verified relay history allows. A waiting
// stage remains durable in IndexedDB and can be retried after either device
// comes online; the fragment is never placed in localStorage.
export async function advanceJoin(wasm, fragment) {
  const invitation = await InvitationStore.open(wasm, fragment, invitationDatabase);
  const publicStore = await PublicStore.open(wasm, publicDatabase);
  try {
    if (!await invitation.isApproved()) return { stage: 'confirmJoin' };
    let claim = await invitation.savedClaim();
    if (!claim || !claim.committedResponse) {
      const progress = await invitation.pull(relayGet, 16);
      if (progress.hasMore) return { stage: 'loadingControls' };
      if (!progress.linkedIssue) return { stage: 'waitingInvite' };
      claim = claim ? await invitation.reconcileCandidate('claim') :
        await invitation.prepareClaim();
    }
    if (!claim.committedResponse) {
      const status = await invitation.status(relayGet).catch(() => null);
      if (status >= 3) return { stage: terminalStage(status) };
      if (status === 2) return { stage: 'waitingClaimResult' };
      if (status === 1) claim = await invitation.refreshCandidate('claim');
      await invitation.submitClaim(relayPostControl);
    }
    if ((await invitation.pullPending(relayGet, 16)).hasMore) {
      return { stage: 'loadingControls' };
    }
    let proof = await invitation.savedProof();
    if (proof && !proof.committedResponse) proof = await invitation.reconcileCandidate('proof');
    if (!proof) {
      const verifier = await invitation.load();
      let challenged = false;
      try {
        verifier.challenge_hpke_object_id();
        challenged = true;
      } catch { /* The holder has not published a challenge yet. */ }
      finally { verifier.free(); }
      if (!challenged) return { stage: 'waitingChallenge' };
      proof = await invitation.prepareProof(relayGet);
    }
    if (!proof.committedResponse) {
      await invitation.refreshCandidate('proof');
      await invitation.submitProof(relayPostControl);
    }
    if ((await invitation.pullPending(relayGet, 16)).hasMore) {
      return { stage: 'loadingControls' };
    }
    const verifier = await invitation.load();
    let admitted;
    try {
      const saved = await invitation.savedClaim();
      admitted = verifier.has_admission(saved.deviceId);
    } finally { verifier.free(); }
    if (!admitted) return { stage: 'waitingGrant' };
    const ready = await invitation.activateAdmitted(publicStore, relayGet);
    if (ready.loadingHistory) return { stage: 'loadingHistory' };
    await invitation.forget();
    return { stage: 'ready', family: ready.family, cursor: ready.cursor };
  } catch (error) {
    const status = await invitation.status(relayGet).catch(() => null);
    if (status >= 3) return { stage: terminalStage(status) };
    if (status === 2 && !await invitation.savedClaim()) return { stage: 'inviteClaimed' };
    if (status === 2 && !(await invitation.savedClaim())?.committedResponse) {
      return { stage: 'waitingClaimResult' };
    }
    throw error;
  } finally {
    invitation.close();
    publicStore.close();
  }
}

function terminalStage(reason) {
  return ({ 3: 'inviteCanceled', 4: 'inviteExpired', 5: 'inviteInvalidated' })[reason];
}

export async function sharedStatus(wasm, family) {
  const store = await PublicStore.open(wasm, publicDatabase);
  try {
    const removal = await store.removedStatus(family);
    let privateCopy = null;
    if (removal) {
      const local = await LocalStore.open(wasm, localDatabase);
      try { privateCopy = (await local.removalCopy(family, removal.transitionId))?.family || null; }
      finally { local.close(); }
    }
    const pending = await store.pendingInitial(family);
    const queued = await store.queuedInitial(family);
    const history = await store.historyStatus(family);
    const verifier = await store.load(family);
    const credential = await store.initialCredential(family).catch(() => null);
    try { return { cursor: Number(verifier.last_cursor()), pending: !!pending, queued: queued.length,
      familyId: hex(verifier.family_id()), removal, privateCopy,
      knownIncomplete: history.knownIncomplete,
      devices: JSON.parse(verifier.active_devices_json()),
      deviceId: credential ? hex(credential.deviceId) : '' }; }
    finally { verifier.free(); }
  } finally { store.close(); }
}
