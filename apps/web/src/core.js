import init, * as wasm from './generated/babytrack_core_wasm.js';
import wasmUrl from './generated/babytrack_core_wasm_bg.wasm?url';
import { LocalStore } from '../../../core-wasm/web/local-store.js';

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
  return (await open()).families();
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
  const projection = await (await open()).load(family);
  try { return JSON.parse(projection.snapshot_json()); }
  finally { projection.free(); }
}

async function append(family, prepare) {
  const local = await open();
  const projection = await local.load(family);
  try { await local.append(family, prepare(projection)); }
  finally { projection.free(); }
}

export async function addChild(family, name, birthDay, sex) {
  const day = birthDay ? BigInt(Math.floor(new Date(`${birthDay}T12:00:00Z`).getTime() / 86400000)) : undefined;
  await append(family, (projection) => projection.create_child_operation(
    name, day, sex ? Number(sex) : undefined, BigInt(Date.now()),
  ));
}

export async function logActivity(family, child, type, values) {
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
  await append(family, (projection) => projection.log_breast_operation(
    bytes(child), JSON.stringify(segments), BigInt(Date.now()),
  ));
}

export async function editBreastFeed(family, child, activity, segments) {
  await append(family, (projection) => projection.edit_breast_operation(
    bytes(child), bytes(activity), JSON.stringify(segments), BigInt(Date.now()),
  ));
}
