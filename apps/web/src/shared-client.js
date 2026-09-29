import { InvitationStore } from '../../../core-wasm/web/invitation-store.js';
import { PublicStore } from '../../../core-wasm/web/public-store.js';
import { relayGet } from '../../../core-wasm/web/relay-get.js';
import { relayPost, relayPostControl } from '../../../core-wasm/web/relay-post.js';

const invitationDatabase = 'babytrack-preview-invitations-v1';
const publicDatabase = 'babytrack-preview-public-v1';
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

export async function sharedSnapshot(wasm, family) {
  const store = await PublicStore.open(wasm, publicDatabase);
  try {
    const ready = await store.loadInitialReadySaved(family);
    try { return JSON.parse(ready.snapshot_json()); }
    finally { ready.free(); }
  } finally { store.close(); }
}

export async function syncShared(wasm, family) {
  const store = await PublicStore.open(wasm, publicDatabase);
  try {
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

export async function writeShared(wasm, family, action, values) {
  const store = await PublicStore.open(wasm, publicDatabase);
  try {
    const credential = await store.initialCredential(family);
    const ready = await store.loadInitialReadySaved(family);
    let operation;
    try {
      const clock = credential.clock || { wallMs: -1, counter: 0 };
      const prefix = [credential.deviceId, BigInt(clock.wallMs), clock.counter];
      const now = BigInt(Date.now());
      const offset = -new Date().getTimezoneOffset();
      if (action === 'child') {
        operation = ready.create_child_operation(...prefix, values.name,
          values.birthDay, values.sex, now);
      } else if (action === 'diaper') {
        operation = ready.log_diaper_operation(...prefix, bytes(values.child),
          Number(values.kind), now, offset);
      } else if (action === 'bottle') {
        operation = ready.log_bottle_operation(...prefix, bytes(values.child),
          Number(values.ml), Number(values.content), now, offset);
      } else if (action === 'note') {
        operation = ready.log_note_operation(...prefix, bytes(values.child),
          values.note, now, offset);
      } else if (action === 'breast') {
        operation = ready.log_breast_operation(...prefix, bytes(values.child),
          JSON.stringify(values.segments), now);
      } else if (action === 'breast-edit') {
        operation = ready.edit_breast_operation(...prefix, bytes(values.child),
          bytes(values.activity), JSON.stringify(values.segments), now);
      } else throw new Error('Unknown shared action');
      await store.queueInitial(family, operation, clock);
    } finally { ready.free(); }
    return { pending: true };
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

// Each call advances as far as the verified relay history allows. A waiting
// stage remains durable in IndexedDB and can be retried after either device
// comes online; the fragment is never placed in localStorage.
export async function advanceJoin(wasm, fragment) {
  const invitation = await InvitationStore.open(wasm, fragment, invitationDatabase);
  const publicStore = await PublicStore.open(wasm, publicDatabase);
  try {
    let claim = await invitation.savedClaim();
    if (!claim) {
      const progress = await invitation.pull(relayGet, 16);
      if (!progress.linkedIssue) return { stage: 'waitingInvite' };
      claim = await invitation.prepareClaim();
    }
    if (!claim.committedResponse) await invitation.submitClaim(relayPostControl);
    await invitation.pullPending(relayGet, 16);
    let proof = await invitation.savedProof();
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
    if (!proof.committedResponse) await invitation.submitProof(relayPostControl);
    await invitation.pullPending(relayGet, 16);
    const verifier = await invitation.load();
    let admitted;
    try {
      const saved = await invitation.savedClaim();
      admitted = verifier.has_admission(saved.deviceId);
    } finally { verifier.free(); }
    if (!admitted) return { stage: 'waitingGrant' };
    const ready = await invitation.activateAdmitted(publicStore, relayGet);
    await invitation.forget();
    return { stage: 'ready', family: ready.family, cursor: ready.cursor };
  } finally {
    invitation.close();
    publicStore.close();
  }
}

export async function sharedStatus(wasm, family) {
  const store = await PublicStore.open(wasm, publicDatabase);
  try {
    const pending = await store.pendingInitial(family);
    const queued = await store.queuedInitial(family);
    const verifier = await store.load(family);
    try { return { cursor: Number(verifier.last_cursor()), pending: !!pending, queued: queued.length,
      familyId: hex(verifier.family_id()) }; }
    finally { verifier.free(); }
  } finally { store.close(); }
}
