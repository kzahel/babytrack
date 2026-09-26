import Foundation

func bytes(_ hex: String) -> Data {
    precondition(hex.count.isMultiple(of: 2))
    let chars = Array(hex.utf8)
    return Data(stride(from: 0, to: chars.count, by: 2).map { index in
        UInt8(String(decoding: chars[index..<(index + 2)], as: UTF8.self), radix: 16)!
    })
}

func object(_ value: Any) -> [String: Any] { value as! [String: Any] }
func string(_ value: Any) -> String { value as! String }
func expect(_ condition: @autoclosure () throws -> Bool) throws {
    let passed = try condition()
    precondition(passed)
}

@main
struct Smoke {
    static func main() throws {
        let root = CommandLine.arguments[1]
        let negative = object(try JSONSerialization.jsonObject(
            with: Data(contentsOf: URL(fileURLWithPath: root + "/tests/vectors/negative-batch-v1.json"))
        ))
        let full = object(try JSONSerialization.jsonObject(
            with: Data(contentsOf: URL(fileURLWithPath: root + "/tests/vectors/full-wire-v1.json"))
        ))
        let base = object(negative["base"]!)
        let cases = negative["cases"] as! [Any]
        func envelope(_ id: String) -> Data {
            let entry = object(cases.first { object($0)["id"] as? String == id }!)
            return bytes(string(object(entry["input"]!)["envelope_cbor_hex"]!))
        }
        let familyId = bytes(string(base["family_id_hex"]!))
        let relayId = bytes("03396219237f75a64f12aeb7f39723abf400b160c364980a765dac24aeba2464")
        let epochKey = bytes(string(base["epoch_key_hex"]!))
        let signer = try ed25519PublicKey(signingSeed: bytes(string(base["recipient_sign_seed_hex"]!)))
        let childId = bytes("0183f9d0000070008000000000000011")

        let family = try NativeFamily(familyId: familyId)
        try expect(try family.applyEnvelope(
            envelope: envelope("CROSSMINORBYTE01"), relayId: relayId,
            epochKey: epochKey, signerPublicKey: signer, cursor: 1
        ))
        try expect(try family.fieldCbor(recordId: childId, fieldId: 1) == bytes("6442616279"))
        try expect(try family.fieldCbor(recordId: childId, fieldId: 500) == bytes("f4"))
        try expect(try family.lastCursor() == 1)

        for id in ["INERTBYTE01", "PRECREATEBYTE01", "WRONGSCOPEBYTE01", "PREFSBYTE01"] {
            let replay = try NativeFamily(familyId: familyId)
            try expect(try !replay.applyEnvelope(
                envelope: envelope(id), relayId: relayId,
                epochKey: epochKey, signerPublicKey: signer, cursor: 1
            ))
            try expect(try replay.inertCount() == 1)
            try expect(try replay.applyEnvelope(
                envelope: envelope("CROSSMINORBYTE01"), relayId: relayId,
                epochKey: epochKey, signerPublicKey: signer, cursor: 2
            ))
        }

        var wrongKey = epochKey
        wrongKey[0] ^= 1
        let clean = try NativeFamily(familyId: familyId)
        do {
            _ = try clean.applyEnvelope(
                envelope: envelope("CROSSMINORBYTE01"), relayId: relayId,
                epochKey: wrongKey, signerPublicKey: signer, cursor: 1
            )
            fatalError("wrong key accepted")
        } catch {}
        try expect(try clean.lastCursor() == 0)
        var wrongSigner = signer
        wrongSigner[0] ^= 1
        do {
            _ = try clean.applyEnvelope(
                envelope: envelope("CROSSMINORBYTE01"), relayId: relayId,
                epochKey: epochKey, signerPublicKey: wrongSigner, cursor: 1
            )
            fatalError("wrong signer accepted")
        } catch {}
        var wrongRelay = relayId
        wrongRelay[0] ^= 1
        do {
            _ = try clean.applyEnvelope(
                envelope: envelope("CROSSMINORBYTE01"), relayId: wrongRelay,
                epochKey: epochKey, signerPublicKey: signer, cursor: 1
            )
            fatalError("wrong relay accepted")
        } catch {}
        var wrongFamilyId = familyId
        wrongFamilyId[0] ^= 1
        let wrongFamily = try NativeFamily(familyId: wrongFamilyId)
        do {
            _ = try wrongFamily.applyEnvelope(
                envelope: envelope("CROSSMINORBYTE01"), relayId: relayId,
                epochKey: epochKey, signerPublicKey: signer, cursor: 1
            )
            fatalError("wrong Family accepted")
        } catch {}

        let fullCases = full["cases"] as! [Any]
        let genesis = object(fullCases.first { object($0)["id"] as? String == "GENESIS01" }!)
        let fixedBatch = object(fullCases.first { object($0)["id"] as? String == "BATCHBYTE01" }!)
        let fixedSigner = try ed25519PublicKey(signingSeed:
            bytes(string(object(genesis["inputs"]!)["manager_sign_seed_hex"]!)))
        let afterControl = try NativeFamily(familyId: familyId)
        try afterControl.advanceControl(cursor: 1)
        try expect(try afterControl.applyEnvelope(
            envelope: bytes(string(object(fixedBatch["expect"]!)["envelope_cbor_hex"]!)),
            relayId: relayId,
            epochKey: bytes(string(object(fixedBatch["inputs"]!)["epoch_key_hex"]!)),
            signerPublicKey: fixedSigner,
            cursor: 2
        ))
        try expect(try afterControl.fieldCbor(recordId: familyId, fieldId: 1) == bytes("a0"))
        print("Swift fixed encrypted batch, minor field, inertness, and authentication: OK")
    }
}
