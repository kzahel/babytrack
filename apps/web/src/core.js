import init, * as wasm from './generated/babytrack_core_wasm.js';
import wasmUrl from './generated/babytrack_core_wasm_bg.wasm?url';
import { LocalStore } from '../../../core-wasm/web/local-store.js';
import { sharedFamilies, readShared, syncShared, pendingJoins, rememberJoin,
  advanceJoin, approveJoin, dismissJoin, sharedStatus, writeShared, copyRemoved,
  exportSharedReadable } from './shared-client.js';

const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');
const bytes = (value) => Uint8Array.from(value.match(/../g) || [], (pair) => parseInt(pair, 16));
let store;

export async function open() {
  if (!store) {
    await init(wasmUrl);
    store = await LocalStore.open(wasm, 'babytrack-preview-local-v1');
  }
  return store;
}

export async function families() {
  const local = (await (await open()).families()).map((row) => ({ ...row, source: 'local' }));
  const shared = (await sharedFamilies(wasm)).map((row) => ({ ...row, source: 'shared' }));
  return [...local, ...shared];
}

export async function createFamily() {
  const ids = wasm.new_local_ids();
  const family = hex(ids.slice(0, 16));
  const device = hex(ids.slice(16));
  const projection = new wasm.WasmLocalFamily(bytes(family), bytes(device));
  try {
    const initial = projection.create_family_operation(BigInt(Date.now()));
    await (await open()).createFamily(family, device, initial);
  } finally { projection.free(); }
  return family;
}

export async function snapshot(family) {
  await open();
  return JSON.parse(await read(family, (projection) => projection.snapshot_json()));
}

// One read over the local or verified shared projection.
async function read(family, reader) {
  await open();
  if (await isShared(family)) return readShared(wasm, family, reader);
  const projection = await store.load(family);
  try { return reader(projection); }
  finally { projection.free(); }
}

// Totals from the shared core for one local day window, in UTC milliseconds.
export async function daySummary(family, child, window) {
  return JSON.parse(await read(family, (projection) => projection.day_summary_json(bytes(child),
    BigInt(window.startMs), BigInt(window.endMs), BigInt(window.throughMs))));
}

export async function analysisCsv(family) {
  return read(family, (projection) => projection.analysis_csv());
}

async function isShared(family) {
  await open();
  return (await sharedFamilies(wasm)).some((row) => row.family === family);
}

export async function rememberInvitation(value) {
  await open();
  return rememberJoin(wasm, value);
}

export async function pendingInvitations() {
  await open();
  return pendingJoins();
}

export async function continueInvitation(fragment) {
  await open();
  return advanceJoin(wasm, fragment);
}

export async function approveInvitation(fragment) {
  await open();
  return approveJoin(wasm, fragment);
}

export async function dismissInvitation(fragment) {
  await open();
  return dismissJoin(wasm, fragment);
}

export async function syncFamily(family) {
  await open();
  return syncShared(wasm, family);
}

export async function familySyncStatus(family) {
  await open();
  return sharedStatus(wasm, family);
}

export async function copyRemovedFamily(family) {
  await open();
  return copyRemoved(wasm, family);
}

export async function exportFamily(family) {
  await open();
  if (await isShared(family)) return exportSharedReadable(wasm, family);
  const projection = await store.load(family);
  try { return projection.readable_file(BigInt(Date.now())); }
  finally { projection.free(); }
}

export async function restoreFamily(file) {
  if (file.size > 64 * 1024 * 1024) throw new Error('Choose a backup under 64 MB');
  const readable = new Uint8Array(await file.arrayBuffer());
  await open();
  const ids = wasm.new_local_ids();
  const family = hex(ids.slice(0, 16));
  const device = hex(ids.slice(16));
  const restore = new wasm.WasmReadableRestore(
    readable, ids.slice(0, 16), ids.slice(16), BigInt(Date.now()));
  try {
    const operations = Array.from({ length: restore.count() }, (_, index) => restore.operation(index));
    await store.createRestoredFamily(family, device, operations);
  } finally { restore.free(); }
  return family;
}

async function append(family, prepare) {
  const local = await open();
  const projection = await local.load(family);
  try { await local.append(family, prepare(projection)); }
  finally { projection.free(); }
}

// The time-zone offset in effect at an instant, as recorded on events.
export function offsetAt(ms) { return -new Date(ms).getTimezoneOffset(); }

// Save one create or correction. Rust validates the intent and builds the
// operation; a shared Family queues it in the encrypted outbox.
export async function act(family, action) {
  const json = JSON.stringify(action);
  const now = BigInt(Date.now());
  const prepare = (projection) => projection.action_operation(json, now);
  if (await isShared(family)) {
    return writeShared(wasm, family, 'action', { json },
      { deliveryId: crypto.randomUUID(), prepare });
  }
  await append(family, prepare);
  return null;
}
