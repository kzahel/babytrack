const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const bindingPath = process.argv[2];
if (!bindingPath) throw new Error('pass generated wasm-bindgen Node module path');
const { WasmFamily, ed25519_public_key, seal_one } = require(path.resolve(bindingPath));
const vectors = JSON.parse(fs.readFileSync(path.join(__dirname, '../../tests/vectors/negative-batch-v1.json')));
const full = JSON.parse(fs.readFileSync(path.join(__dirname, '../../tests/vectors/full-wire-v1.json')));
const hex = (value) => Buffer.from(value, 'hex');
const familyId = hex(vectors.base.family_id_hex);
const relayId = hex('03396219237f75a64f12aeb7f39723abf400b160c364980a765dac24aeba2464');
const epochKey = hex(vectors.base.epoch_key_hex);
const signer = ed25519_public_key(hex(vectors.base.recipient_sign_seed_hex));
const childId = hex('0183f9d0000070008000000000000011');
const envelope = (id) => hex(vectors.cases.find((entry) => entry.id === id).input.envelope_cbor_hex);
const minorCase = vectors.cases.find((entry) => entry.id === 'CROSSMINORBYTE01');
const sealedChild = seal_one(
  hex(minorCase.input.header_cbor_hex), hex(minorCase.input.operation_hex),
  epochKey, hex(vectors.base.recipient_sign_seed_hex),
);
assert.equal(Buffer.from(sealedChild).toString('hex'), minorCase.input.envelope_cbor_hex);

const family = new WasmFamily(familyId);
assert.equal(family.apply_envelope(sealedChild, relayId, epochKey, signer, 1n), true);
assert.equal(Buffer.from(family.field_cbor(childId, 1n)).toString('hex'), '6442616279');
assert.equal(Buffer.from(family.field_cbor(childId, 500n)).toString('hex'), 'f4');
assert.equal(family.last_cursor(), 1n);
assert.equal(family.inert_count(), 0);

for (const id of ['INERTBYTE01', 'PRECREATEBYTE01', 'WRONGSCOPEBYTE01', 'PREFSBYTE01']) {
  const replay = new WasmFamily(familyId);
  assert.equal(replay.apply_envelope(envelope(id), relayId, epochKey, signer, 1n), false, id);
  assert.equal(replay.last_cursor(), 1n, id);
  assert.equal(replay.inert_count(), 1, id);
  assert.equal(replay.apply_envelope(sealedChild, relayId, epochKey, signer, 2n), true, id);
  assert.equal(Buffer.from(replay.field_cbor(childId, 500n)).toString('hex'), 'f4', id);
}

const wrongKey = Buffer.from(epochKey);
wrongKey[0] ^= 1;
const clean = new WasmFamily(familyId);
assert.throws(() => clean.apply_envelope(envelope('CROSSMINORBYTE01'), relayId, wrongKey, signer, 1n));
assert.equal(clean.last_cursor(), 0n);
const wrongSigner = Buffer.from(signer);
wrongSigner[0] ^= 1;
assert.throws(() => clean.apply_envelope(envelope('CROSSMINORBYTE01'), relayId, epochKey, wrongSigner, 1n));
assert.equal(clean.last_cursor(), 0n);
const tamperedSignature = Buffer.from(envelope('CROSSMINORBYTE01'));
tamperedSignature[tamperedSignature.length - 1] ^= 1;
assert.throws(() => clean.apply_envelope(tamperedSignature, relayId, epochKey, signer, 1n));
assert.equal(clean.last_cursor(), 0n);
const wrongRelay = Buffer.from(relayId);
wrongRelay[0] ^= 1;
assert.throws(() => clean.apply_envelope(envelope('CROSSMINORBYTE01'), wrongRelay, epochKey, signer, 1n));
const wrongFamilyId = Buffer.from(familyId);
wrongFamilyId[0] ^= 1;
assert.throws(() => new WasmFamily(wrongFamilyId)
  .apply_envelope(envelope('CROSSMINORBYTE01'), relayId, epochKey, signer, 1n));

const genesis = full.cases.find((entry) => entry.id === 'GENESIS01');
const fixedBatch = full.cases.find((entry) => entry.id === 'BATCHBYTE01');
const fixedSigner = ed25519_public_key(hex(genesis.inputs.manager_sign_seed_hex));
assert.equal(Buffer.from(fixedSigner).toString('hex'), genesis.expect.manager_sign_public_key_hex);
const sealed = seal_one(
  hex(fixedBatch.expect.header_cbor_hex),
  hex(fixedBatch.inputs.operation_cbor_hex),
  hex(fixedBatch.inputs.epoch_key_hex),
  hex(genesis.inputs.manager_sign_seed_hex),
);
assert.equal(Buffer.from(sealed).toString('hex'), fixedBatch.expect.envelope_cbor_hex);
const afterControl = new WasmFamily(familyId);
afterControl.advance_control(1n);
assert.equal(afterControl.apply_envelope(
  sealed, relayId,
  hex(fixedBatch.inputs.epoch_key_hex), fixedSigner, 2n,
), true);
assert.equal(Buffer.from(afterControl.field_cbor(familyId, 1n)).toString('hex'), 'a0');

console.log('wasm fixed encrypted batch, minor field, inertness, and authentication: OK');
