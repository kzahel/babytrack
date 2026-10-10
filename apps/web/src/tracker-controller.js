import { writable } from 'svelte/store';

const emptyData = () => ({ children: [], activities: [] });

// UI coordination only. Verification, durable operations and enrollment remain
// in the core adapters. Every async publication names its original selection.
export function createTrackerController({ api, copy: c, preferences, onSelect = () => {},
  onRemoval = () => {}, onRememberInvitation = () => {} }) {
  let state = { loading: true, familyRows: [], family: '', child: '', data: emptyData(),
    pendingFragment: '', joinStage: '', syncStage: '', removedInfo: null,
    privateCopy: '', backupGap: false, backupCursor: 0, devices: [], deviceId: '' };
  const store = writable(state);
  let generation = 0;
  let refreshSequence = 0;
  let polling = false;
  let disposed = false;
  const publish = (patch) => {
    if (!disposed) { state = { ...state, ...patch }; store.set(state); }
  };
  const target = () => ({ family: state.family, child: state.child, generation });
  const current = (saved) => !disposed && saved.generation === generation && saved.family === state.family;
  const shared = (family) => state.familyRows.some((row) => row.family === family && row.source === 'shared');
  const statusPatch = (status) => ({ removedInfo: status?.removal || null,
    privateCopy: status?.privateCopy || '', backupGap: !!(status?.knownIncomplete || status?.removal?.knownGap),
    backupCursor: status?.cursor || 0, devices: status?.devices || [], deviceId: status?.deviceId || '',
    syncStage: !status ? '' : status.removal ? (status.privateCopy ? c.removedCopied : c.removedArchive) :
      status.pending || status.queued ? c.savedPending : status.knownIncomplete ? c.syncMore : c.syncReady });

  function choose(family) {
    generation++;
    preferences.setItem('babytrack-family', family);
    publish({ family, data: emptyData(), loading: !!family, ...statusPatch(null) });
    if (!disposed) onSelect();
    return target();
  }
  async function refresh(saved = target()) {
    const sequence = ++refreshSequence;
    if (!saved.family) return false;
    const data = await api.snapshot(saved.family);
    const status = shared(saved.family) ? await api.familySyncStatus(saved.family) : null;
    if (!current(saved) || sequence !== refreshSequence) return false;
    const child = data.children.find((row) => row.id === state.child)?.id || data.children[0]?.id || '';
    publish({ data, child, loading: false, ...statusPatch(status) });
    return true;
  }
  async function selectFamily(family) {
    const saved = choose(family);
    try { return await refresh(saved); }
    finally { if (current(saved)) publish({ loading: false }); }
  }
  function selectChild(child) {
    generation++;
    publish({ child });
  }
  async function initialize(fragment) {
    try {
      if (fragment.startsWith('#bt-invite=')) {
        publish({ pendingFragment: await api.rememberInvitation(fragment) });
        if (!disposed) onRememberInvitation();
      }
      if (!state.pendingFragment) publish({ pendingFragment: (await api.pendingInvitations())[0] || '' });
      const familyRows = await api.families();
      if (disposed) return;
      publish({ familyRows });
      const preferred = preferences.getItem('babytrack-family');
      await selectFamily(familyRows.find((row) => row.family === preferred)?.family || familyRows[0]?.family || '');
      publish({ loading: false });
      await poll();
    } catch (cause) { publish({ loading: false }); throw cause; }
  }
  async function poll() {
    if (polling || disposed) return;
    polling = true;
    let saved = target();
    const fragment = state.pendingFragment;
    try {
      if (fragment) {
        publish({ joinStage: c.joining });
        const progress = await api.continueInvitation(fragment);
        if (!current(saved) || state.pendingFragment !== fragment) return;
        publish({ joinStage: c[progress.stage] || c.joining });
        if (progress.stage === 'ready') {
          const familyRows = await api.families();
          if (!current(saved) || state.pendingFragment !== fragment) return;
          publish({ familyRows, pendingFragment: '', joinStage: '' });
          await selectFamily(progress.family);
          saved = target();
        }
      }
      if (saved.family && shared(saved.family)) {
        const progress = await api.syncFamily(saved.family);
        if (!current(saved)) return;
        if (progress.removed && progress.privateCopy) {
          const familyRows = await api.families();
          if (!current(saved)) return;
          publish({ familyRows });
        }
        if (await refresh(saved) && progress.removed) onRemoval();
      }
    } catch (cause) {
      if (!current(saved)) return;
      if (fragment && state.pendingFragment === fragment) {
        publish({ joinStage: `${c.joinPending} · ${cause?.message || String(cause)}` });
      } else {
        const status = saved.family && shared(saved.family) ?
          await api.familySyncStatus(saved.family).catch(() => null) : null;
        if (current(saved)) publish({ syncStage: status?.pending || status?.queued ? c.savedPending : c.syncFailed });
      }
    } finally { polling = false; }
  }
  async function startJoin(value) {
    const fragment = await api.rememberInvitation(value);
    publish({ pendingFragment: fragment });
    await api.approveInvitation(fragment);
    if (state.pendingFragment === fragment) { publish({ joinStage: c.joining }); await poll(); }
  }
  async function confirmJoin() {
    const fragment = state.pendingFragment;
    await api.approveInvitation(fragment);
    if (state.pendingFragment === fragment) { publish({ joinStage: c.joining }); await poll(); }
  }
  async function dismissJoin() {
    const fragment = state.pendingFragment;
    await api.dismissInvitation(fragment);
    if (state.pendingFragment === fragment) publish({ pendingFragment: '', joinStage: '' });
  }
  async function afterSave(outcome, saved) {
    if (!current(saved)) return false;
    if (outcome?.redirectFamily) {
      const familyRows = await api.families();
      if (!current(saved)) return false;
      publish({ familyRows });
      return selectFamily(outcome.redirectFamily);
    }
    const applied = await refresh(saved);
    if (applied && shared(saved.family)) { publish({ syncStage: c.savedPending }); void poll(); }
    return applied;
  }
  async function createAndSelect(create) {
    const saved = target();
    const family = await create();
    const familyRows = await api.families();
    if (!current(saved)) return false;
    publish({ familyRows });
    return selectFamily(family);
  }
  async function makeRemovedCopy() {
    const saved = target();
    const privateCopy = await api.copyRemovedFamily(saved.family);
    const familyRows = await api.families();
    if (current(saved)) publish({ familyRows, privateCopy, syncStage: c.removedCopied });
  }
  return { subscribe: store.subscribe, target, current, initialize, refresh, poll,
    selectFamily, selectChild, startJoin, confirmJoin, dismissJoin, afterSave, makeRemovedCopy,
    makeFamily: () => createAndSelect(() => api.createFamily()),
    restoreBackup: (file) => createAndSelect(() => api.restoreFamily(file)),
    dispose: () => { disposed = true; generation++; } };
}
