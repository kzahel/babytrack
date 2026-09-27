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
        let minorCase = object(cases.first { object($0)["id"] as? String == "CROSSMINORBYTE01" }!)
        let minorInput = object(minorCase["input"]!)
        let sealedChild = try sealOne(
            headerCbor: bytes(string(minorInput["header_cbor_hex"]!)),
            operationCbor: bytes(string(minorInput["operation_hex"]!)),
            epochKey: epochKey,
            signingSeed: bytes(string(base["recipient_sign_seed_hex"]!))
        )
        try expect(sealedChild == bytes(string(minorInput["envelope_cbor_hex"]!)))

        let family = try NativeFamily(familyId: familyId)
        try expect(try family.applyEnvelope(
            envelope: sealedChild, relayId: relayId,
            epochKey: epochKey, signerPublicKey: signer, cursor: 1
        ))
        try expect(try family.fieldCbor(recordId: childId, fieldId: 1) == bytes("6442616279"))
        try expect(try family.fieldCbor(recordId: childId, fieldId: 500) == bytes("f4"))
        try expect(try family.lastCursor() == 1)

        for id in ["INERTBYTE01", "PRECREATEBYTE01", "SETTHENCREATEBYTE01", "WRONGSCOPEBYTE01", "PREFSBYTE01"] {
            let replay = try NativeFamily(familyId: familyId)
            try expect(try !replay.applyEnvelope(
                envelope: envelope(id), relayId: relayId,
                epochKey: epochKey, signerPublicKey: signer, cursor: 1
            ))
            try expect(try replay.inertCount() == 1)
            if id == "PRECREATEBYTE01" || id == "SETTHENCREATEBYTE01" {
                try expect(try replay.fieldCbor(recordId: bytes("0183f9d0000070008000000000000021"), fieldId: 1).isEmpty)
            }
            try expect(try replay.applyEnvelope(
                envelope: sealedChild, relayId: relayId,
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
        var tamperedSignature = envelope("CROSSMINORBYTE01")
        tamperedSignature[tamperedSignature.count - 1] ^= 1
        do {
            _ = try clean.applyEnvelope(
                envelope: tamperedSignature, relayId: relayId,
                epochKey: epochKey, signerPublicKey: signer, cursor: 1
            )
            fatalError("tampered signature accepted")
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
        let sealed = try sealOne(
            headerCbor: bytes(string(object(fixedBatch["expect"]!)["header_cbor_hex"]!)),
            operationCbor: bytes(string(object(fixedBatch["inputs"]!)["operation_cbor_hex"]!)),
            epochKey: bytes(string(object(fixedBatch["inputs"]!)["epoch_key_hex"]!)),
            signingSeed: bytes(string(object(genesis["inputs"]!)["manager_sign_seed_hex"]!))
        )
        try expect(sealed == bytes(string(object(fixedBatch["expect"]!)["envelope_cbor_hex"]!)))
        let afterControl = try NativeFamily(familyId: familyId)
        try afterControl.advanceControl(cursor: 1)
        try expect(try afterControl.applyEnvelope(
            envelope: sealed,
            relayId: relayId,
            epochKey: bytes(string(object(fixedBatch["inputs"]!)["epoch_key_hex"]!)),
            signerPublicKey: fixedSigner,
            cursor: 2
        ))
        try expect(try afterControl.fieldCbor(recordId: familyId, fieldId: 1) == bytes("a0"))
        let phonePath = FileManager.default.temporaryDirectory
            .appendingPathComponent("babytrack-native-swift-\(UUID().uuidString).db").path
        let phone = try NativeLocalStore.open(path: phonePath)
        let first = try phone.createFamily(nowMs: 1_790_000_000_000)
        let other = try phone.createFamily(nowMs: 1_790_000_000_001)
        let child = try phone.addChild(family: first, name: "Baby", nowMs: 1_790_000_000_002)
        try expect(try phone.children(family: first).first?.name == "Baby")
        let whenRecorded = ActivityWhen(
            startUtcMs: 1_790_000_000_003, offsetMinutes: 120,
            savedAtMs: 1_790_000_000_004)
        do {
            _ = try phone.logDiaper(family: other, childId: child, kind: 1, time: whenRecorded)
            fatalError("cross-Family child accepted")
        } catch {}
        _ = try phone.logDiaper(family: first, childId: child, kind: 3, time: whenRecorded)
        _ = try phone.logBottleMl(
            family: first, childId: child, amountMl: 85,
            content: 2, time: whenRecorded)
        let entries = try phone.timeline(family: first, childId: child)
        try expect(entries.count == 2)
        try expect(entries.contains { $0.diaperKind == 3 })
        try expect(entries.contains { $0.bottleMl == 85 })
        let backup = try phone.backup(family: first, nowMs: 1_790_000_000_005)
        let restored = try phone.restore(bytes: backup, nowMs: 1_790_000_000_006)
        try expect(restored.familyId != first.familyId)
        try expect(try phone.timeline(family: restored, childId: child).count == 2)
        let reopened = try NativeLocalStore.open(path: phonePath)
        try expect(try reopened.families().count == 3)
        print("Swift fixed encrypted batch, minor field, inertness, and authentication: OK")
    }
}
