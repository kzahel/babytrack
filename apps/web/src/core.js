import init, * as wasm from './generated/babytrack_core_wasm.js';
import wasmUrl from './generated/babytrack_core_wasm_bg.wasm?url';
import { LocalStore } from '../../../core-wasm/web/local-store.js';
import { sharedFamilies, sharedSnapshot, syncShared, pendingJoins, rememberJoin,
  advanceJoin, sharedStatus, writeShared } from './shared-client.js';

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

export async function syncFamily(family) {
  await open();
  return syncShared(wasm, family);
}

export async function familySyncStatus(family) {
  await open();
  return sharedStatus(wasm, family);
}

async function append(family, prepare) {
  const local = await open();
  const projection = await local.load(family);
  try { await local.append(family, prepare(projection)); }
  finally { projection.free(); }
}

export async function addChild(family, name, birthDay, sex) {
  const day = birthDay ? BigInt(Math.floor(new Date(`${birthDay}T12:00:00Z`).getTime() / 86400000)) : undefined;
  if (await isShared(family)) {
    await writeShared(wasm, family, 'child', { name, birthDay: day,
      sex: sex ? Number(sex) : undefined });
    return;
  }
  await append(family, (projection) => projection.create_child_operation(
    name, day, sex ? Number(sex) : undefined, BigInt(Date.now()),
  ));
}

export async function logActivity(family, child, type, values) {
  if (await isShared(family)) {
    await writeShared(wasm, family, type, { child, ...values });
    return;
  }
  const id = bytes(child);
  const now = BigInt(Date.now());
  const offset = -new Date().getTimezoneOffset();
  await append(family, (projection) => {
    if (type === 'diaper') return projection.log_diaper_operation(id, Number(values.kind), now, offset);
    if (type === 'bottle') return projection.log_bottle_operation(id, Number(values.ml), Number(values.content), now, offset);
    if (type === 'note') return projection.log_note_operation(id, values.note, now, offset);
    throw new Error('Unknown activity');
  });
}

export async function logBreastFeed(family, child, segments) {
  if (await isShared(family)) {
    await writeShared(wasm, family, 'breast', { child, segments });
    return;
  }
  await append(family, (projection) => projection.log_breast_operation(
    bytes(child), JSON.stringify(segments), BigInt(Date.now()),
  ));
}

export async function editBreastFeed(family, child, activity, segments) {
  if (await isShared(family)) {
    await writeShared(wasm, family, 'breast-edit', { child, activity, segments });
    return;
  }
  await append(family, (projection) => projection.edit_breast_operation(
    bytes(child), bytes(activity), JSON.stringify(segments), BigInt(Date.now()),
  ));
}
