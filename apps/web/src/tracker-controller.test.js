import test from 'node:test';
import assert from 'node:assert/strict';
import { get } from 'svelte/store';
import { createTrackerController } from './tracker-controller.js';

const deferred = () => {
  let resolve;
  const promise = new Promise((done) => { resolve = done; });
  return { promise, resolve };
};
const snapshot = (family) => ({ children: [{ id: `${family}-child`, name: family }], activities: [] });
function setup(overrides = {}, rows = [{ family: 'A', source: 'local' }, { family: 'B', source: 'local' }]) {
  const preferences = new Map();
  const tracker = createTrackerController({
    api: { families: async () => rows, pendingInvitations: async () => [],
      snapshot: async (family) => snapshot(family), ...overrides },
    copy: { savedPending: 'pending', syncReady: 'ready', removedCopied: 'copied' },
    preferences: { getItem: (key) => preferences.get(key), setItem: (key, value) => preferences.set(key, value) },
  });
  return tracker;
}

test('a delayed Family load cannot replace the newly selected Family', async () => {
  const a = deferred();
  const tracker = setup({ snapshot: (family) => family === 'A' ? a.promise : Promise.resolve(snapshot(family)) });
  const oldLoad = tracker.selectFamily('A');
  await tracker.selectFamily('B');
  a.resolve(snapshot('A'));
  assert.equal(await oldLoad, false);
  assert.equal(get(tracker).family, 'B');
  assert.equal(get(tracker).child, 'B-child');
  assert.equal(get(tracker).data.children[0].name, 'B');
});

test('shared data and status publish together, and a late poll cannot affect another Family', async () => {
  const sync = deferred();
  const status = deferred();
  const tracker = setup({
    syncFamily: () => sync.promise,
    familySyncStatus: () => status.promise,
  }, [{ family: 'A', source: 'shared' }, { family: 'B', source: 'local' }]);
  const seen = [];
  const unsubscribe = tracker.subscribe((state) => seen.push(state));
  const initial = tracker.initialize('');
  await new Promise((done) => setImmediate(done));
  assert.equal(get(tracker).data.children.length, 0);
  status.resolve({ cursor: 4, pending: 1 });
  await new Promise((done) => setImmediate(done));
  assert.ok(seen.some((state) => state.data.children[0]?.name === 'A' && state.backupCursor === 4));
  await tracker.selectFamily('B');
  sync.resolve({ removed: true, privateCopy: 'copy-A' });
  await initial;
  assert.equal(get(tracker).family, 'B');
  assert.equal(get(tracker).syncStage, '');
  assert.equal(get(tracker).removedInfo, null);
  assert.equal(get(tracker).privateCopy, '');
  unsubscribe();
});

test('older refresh of the same Family cannot replace a newer snapshot', async () => {
  const old = deferred();
  let calls = 0;
  const tracker = setup({ snapshot: () => ++calls === 1 ? old.promise : Promise.resolve(snapshot('new')) });
  const pending = tracker.selectFamily('A');
  assert.equal(await tracker.refresh(), true);
  old.resolve(snapshot('old'));
  await pending;
  assert.equal(get(tracker).data.children[0].name, 'new');
});

test('save completion retains its original target after a selection switch', async () => {
  const savedSnapshot = deferred();
  let holdNextSnapshot = false;
  const tracker = setup({ snapshot: async (family) => {
    if (holdNextSnapshot) { holdNextSnapshot = false; return savedSnapshot.promise; }
    return snapshot(family);
  } });
  await tracker.selectFamily('A');
  holdNextSnapshot = true;
  const completion = tracker.afterSave({}, tracker.target());
  await tracker.selectFamily('B');
  savedSnapshot.resolve(snapshot('A'));
  assert.equal(await completion, false);
  assert.equal(get(tracker).family, 'B');
});

test('a background refresh cannot make a committed save appear unsuccessful', async () => {
  const savedSnapshot = deferred();
  let holdNextSnapshot = false;
  let data = snapshot('A');
  const tracker = setup({
    snapshot: async () => {
      if (holdNextSnapshot) { holdNextSnapshot = false; return savedSnapshot.promise; }
      return data;
    },
    syncFamily: async () => ({}),
    familySyncStatus: async () => ({ pending: 1 }),
  }, [{ family: 'A', source: 'shared' }]);
  await tracker.initialize('');
  const savedData = { ...data, activities: [{ id: 'saved-note', note: 'Pending note' }] };
  data = { ...savedData, activities: [...savedData.activities, { id: 'remote-note' }] };
  holdNextSnapshot = true;
  const completion = tracker.afterSave({}, tracker.target());
  await tracker.poll();
  savedSnapshot.resolve(savedData);
  assert.equal(await completion, true, 'the form must finish after the committed save');
  assert.deepEqual(get(tracker).data.activities.map((row) => row.id), ['saved-note', 'remote-note']);
  assert.equal(get(tracker).syncStage, 'pending');
});

test('save completion waits for an in-flight superseding snapshot', async () => {
  const saveRead = deferred();
  const pollRead = deferred();
  let reads = null;
  const tracker = setup({
    snapshot: async () => reads ? reads.shift().promise : snapshot('A'),
    syncFamily: async () => ({}),
    familySyncStatus: async () => ({ pending: 1 }),
  }, [{ family: 'A', source: 'shared' }]);
  await tracker.initialize('');
  reads = [saveRead, pollRead];
  let finished = false;
  const completion = tracker.afterSave({}, tracker.target()).then((result) => {
    finished = true;
    return result;
  });
  const poll = tracker.poll();
  await new Promise((done) => setImmediate(done));
  const savedData = { ...snapshot('A'), activities: [{ id: 'saved-note' }] };
  saveRead.resolve(savedData);
  await new Promise((done) => setImmediate(done));
  assert.equal(finished, false, 'follow-up UI needs the saved entry in its current snapshot');
  pollRead.resolve(savedData);
  assert.equal(await completion, true);
  await poll;
  assert.equal(get(tracker).data.activities[0].id, 'saved-note');
});

test('disposing the route prevents an in-flight load from publishing', async () => {
  const old = deferred();
  const tracker = setup({ snapshot: () => old.promise });
  const pending = tracker.selectFamily('A');
  tracker.dispose();
  old.resolve(snapshot('A'));
  assert.equal(await pending, false);
  assert.equal(get(tracker).data.children.length, 0);
});

test('a removal redirect keeps the saved child in the independent copy', async () => {
  const tracker = setup({ snapshot: async () => ({ children: [
    { id: 'first-child' }, { id: 'saved-child' },
  ], activities: [] }) }, [{ family: 'A', source: 'local' }, { family: 'copy-A', source: 'local' }]);
  await tracker.initialize('');
  tracker.selectChild('saved-child');
  assert.equal(await tracker.afterSave({ redirectFamily: 'copy-A' }, tracker.target()), true);
  assert.equal(get(tracker).family, 'copy-A');
  assert.equal(get(tracker).child, 'saved-child');
});
