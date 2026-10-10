import init, * as wasm from './generated/babytrack_core_wasm.js';
import wasmUrl from './generated/babytrack_core_wasm_bg.wasm?url';
import { LocalStore } from '../../../core-wasm/web/local-store.js';
import { sharedFamilies, sharedSnapshot, syncShared, pendingJoins, rememberJoin,
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
  if (await isShared(family)) {
    return sharedSnapshot(wasm, family);
  }
  const projection = await (await open()).load(family);
  try { return JSON.parse(projection.snapshot_json()); }
  finally { projection.free(); }
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

export async function addChild(family, name, birthDay, sex) {
  const day = birthDay ? BigInt(Math.floor(new Date(`${birthDay}T12:00:00Z`).getTime() / 86400000)) : undefined;
  const now = BigInt(Date.now());
  const prepare = (projection) => projection.create_child_operation(
    name, day, sex ? Number(sex) : undefined, now);
  if (await isShared(family)) {
    const result = await writeShared(wasm, family, 'child', { name, birthDay: day,
      sex: sex ? Number(sex) : undefined },
    { deliveryId: crypto.randomUUID(), prepare });
    if (!result.redirectFamily) return result;
    return result;
  }
  await append(family, prepare);
  return null;
}

export async function logActivity(family, child, type, values) {
  const id = bytes(child);
  const now = BigInt(Date.now());
  const offset = -new Date().getTimezoneOffset();
  const prepare = (projection) => {
    if (type === 'diaper') return projection.log_diaper_operation(id, Number(values.kind), now, offset);
    if (type === 'bottle') return projection.log_bottle_operation(id, Number(values.ml), Number(values.content), now, offset);
    if (type === 'note') return projection.log_note_operation(id, values.note, now, offset);
    throw new Error('Unknown activity');
  };
  if (await isShared(family)) {
    const result = await writeShared(wasm, family, type, { child, ...values },
      { deliveryId: crypto.randomUUID(), prepare });
    if (!result.redirectFamily) return result;
    return result;
  }
  await append(family, prepare);
  return null;
}

export async function logBreastFeed(family, child, segments) {
  const now = BigInt(Date.now());
  const prepare = (projection) => projection.log_breast_operation(
    bytes(child), JSON.stringify(segments), now);
  if (await isShared(family)) {
    const result = await writeShared(wasm, family, 'breast', { child, segments },
      { deliveryId: crypto.randomUUID(), prepare });
    if (!result.redirectFamily) return result;
    return result;
  }
  await append(family, prepare);
  return null;
}

export async function startSleep(family, child) {
  const now = BigInt(Date.now());
  const offset = -new Date().getTimezoneOffset();
  const prepare = (projection) => projection.start_sleep_operation(bytes(child), now, offset);
  if (await isShared(family)) {
    return writeShared(wasm, family, 'sleep-start', { child },
      { deliveryId: crypto.randomUUID(), prepare });
  }
  await append(family, prepare);
  return null;
}

export async function stopSleep(family, child, activity) {
  const now = BigInt(Date.now());
  const offset = -new Date().getTimezoneOffset();
  const prepare = (projection) => projection.stop_sleep_operation(
    bytes(child), bytes(activity), now, offset);
  if (await isShared(family)) {
    return writeShared(wasm, family, 'sleep-stop', { child, activity },
      { deliveryId: crypto.randomUUID(), prepare });
  }
  await append(family, prepare);
  return null;
}

export async function editBreastFeed(family, child, activity, segments) {
  const now = BigInt(Date.now());
  const prepare = (projection) => projection.edit_breast_operation(
    bytes(child), bytes(activity), JSON.stringify(segments), now);
  if (await isShared(family)) {
    const result = await writeShared(wasm, family, 'breast-edit', { child, activity, segments },
      { deliveryId: crypto.randomUUID(), prepare });
    if (!result.redirectFamily) return result;
    return result;
  }
  await append(family, prepare);
  return null;
}
