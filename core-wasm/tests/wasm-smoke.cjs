const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const bindingPath = process.argv[2];
if (!bindingPath) throw new Error('pass generated wasm-bindgen Node module path');
const { WasmFamily, WasmPublicFamily, WasmInitialFamily, WasmLogPage,
  ed25519_public_key, seal_one } = require(path.resolve(bindingPath));
const vectors = JSON.parse(fs.readFileSync(path.join(__dirname, '../../tests/vectors/negative-batch-v1.json')));
const full = JSON.parse(fs.readFileSync(path.join(__dirname, '../../tests/vectors/full-wire-v1.json')));
const api = JSON.parse(fs.readFileSync(path.join(__dirname, '../../tests/vectors/api-v1.json')));
const chain = JSON.parse(fs.readFileSync(path.join(__dirname, '../../tests/vectors/contiguous-chain-v1.json')));
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

for (const id of ['INERTBYTE01', 'PRECREATEBYTE01', 'SETTHENCREATEBYTE01', 'WRONGSCOPEBYTE01', 'PREFSBYTE01']) {
  const replay = new WasmFamily(familyId);
  assert.equal(replay.apply_envelope(envelope(id), relayId, epochKey, signer, 1n), false, id);
  assert.equal(replay.last_cursor(), 1n, id);
  assert.equal(replay.inert_count(), 1, id);
  if (id === 'PRECREATEBYTE01' || id === 'SETTHENCREATEBYTE01') {
    assert.equal(replay.field_cbor(hex('0183f9d0000070008000000000000021'), 1n).length, 0, id);
  }
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
const publicFamily = new WasmPublicFamily(
  hex(genesis.expect.control_object_hex), hex(genesis.expect.relay_public_key_hex),
);
assert.equal(Buffer.from(publicFamily.family_id()).toString('hex'), genesis.inputs.family_id_hex);
assert.equal(Buffer.from(publicFamily.head_hash()).toString('hex'), genesis.expect.control_head_hex);
publicFamily.apply_batch(sealed, hex(fixedBatch.expect.accepted_receipt_cbor_hex));
assert.equal(publicFamily.last_cursor(), 2n);
const ready = new WasmInitialFamily(
  hex(genesis.expect.control_object_hex), hex(genesis.expect.relay_public_key_hex),
  hex(genesis.inputs.epoch_key_hex),
);
assert.throws(() => ready.finish());
ready.add_object(hex(genesis.inputs.promotion_id_hex),
  hex(genesis.expect.promotion_manifest_object_hex));
ready.finish();
assert.equal(ready.last_cursor(), 1n);
assert.equal(ready.record_type(hex(genesis.inputs.family_id_hex)), undefined);
assert.equal(ready.apply_batch(sealed, hex(fixedBatch.expect.accepted_receipt_cbor_hex)), true);
assert.equal(ready.last_cursor(), 2n);
assert.equal(ready.record_type(hex(genesis.inputs.family_id_hex)), 'family');
const badObject = new WasmInitialFamily(
  hex(genesis.expect.control_object_hex), hex(genesis.expect.relay_public_key_hex),
  hex(genesis.inputs.epoch_key_hex),
);
const damagedPromotion = hex(genesis.expect.promotion_manifest_object_hex);
damagedPromotion[damagedPromotion.length - 1] ^= 1;
badObject.add_object(hex(genesis.inputs.promotion_id_hex), damagedPromotion);
assert.throws(() => badObject.finish());
assert.throws(() => badObject.apply_batch(sealed, hex(fixedBatch.expect.accepted_receipt_cbor_hex)));
badObject.free();
const wrongInitialKey = hex(genesis.inputs.epoch_key_hex);
wrongInitialKey[0] ^= 1;
assert.throws(() => new WasmInitialFamily(
  hex(genesis.expect.control_object_hex), hex(genesis.expect.relay_public_key_hex), wrongInitialKey,
));
ready.free();
const denied = new WasmPublicFamily(
  hex(genesis.expect.control_object_hex), hex(genesis.expect.relay_public_key_hex),
);
const alteredReceipt = hex(fixedBatch.expect.accepted_receipt_cbor_hex);
alteredReceipt[alteredReceipt.length - 1] ^= 1;
assert.throws(() => denied.apply_batch(sealed, alteredReceipt));
assert.equal(denied.last_cursor(), 1n);
const issue = hex(chain.transitions[1].committed_cbor_hex);
const issueLength = Buffer.alloc(3);
issueLength[0] = 0x59;
issueLength.writeUInt16BE(issue.length, 1);
const pageBytes = Buffer.concat([
  Buffer.from([0xa6, 1, 1, 2, 0x50]), hex(chain.test_only_inputs.family_id_hex),
  Buffer.from([3, 1, 4, 0x81, 0x83, 2, 1]), issueLength, issue,
  Buffer.from([5, 2, 6, 0xf4]),
]);
const page = new WasmLogPage(pageBytes,
  hex(chain.test_only_inputs.family_id_hex), 1n);
assert.equal(page.len(), 1);
assert.equal(page.entry_kind(0), 1);
assert.equal(page.entry_cursor(0), 2n);
const controlFamily = new WasmPublicFamily(
  hex(chain.transitions[0].committed_cbor_hex), hex(genesis.expect.relay_public_key_hex),
);
const readAuth = controlFamily.sign_get(
  hex(chain.test_only_inputs.manager_device_id_hex), hex(chain.test_only_inputs.manager_sign_seed_hex),
  api.inputs.read_control_path, hex('0102030405060708090a0b0c0d0e0f10'),
);
assert.ok(readAuth.length > 64);
assert.throws(() => controlFamily.sign_get(
  hex(chain.test_only_inputs.manager_device_id_hex), hex(chain.test_only_inputs.manager_sign_seed_hex),
  '/v1/families/ffffffffffffffffffffffffffffffff/log?after=1',
  hex('0102030405060708090a0b0c0d0e0f10'),
));
controlFamily.apply_control(page.entry_bytes(0));
assert.equal(controlFamily.last_cursor(), 2n);
page.free();
controlFamily.free();
publicFamily.free();
denied.free();
const afterControl = new WasmFamily(familyId);
afterControl.advance_control(1n);
assert.equal(afterControl.apply_envelope(
  sealed, relayId,
  hex(fixedBatch.inputs.epoch_key_hex), fixedSigner, 2n,
), true);
assert.equal(Buffer.from(afterControl.field_cbor(familyId, 1n)).toString('hex'), 'a0');

console.log('wasm fixed encrypted batch, minor field, inertness, and authentication: OK');
