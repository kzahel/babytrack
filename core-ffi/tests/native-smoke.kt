import com.google.gson.JsonObject
import com.google.gson.JsonParser
import java.nio.file.Files
import java.nio.file.Path
import uniffi.babytrack_core_ffi.NativeFamily
import uniffi.babytrack_core_ffi.ed25519PublicKey
import uniffi.babytrack_core_ffi.sealOne

private fun bytes(hex: String): ByteArray = hex.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
private fun ByteArray.hex(): String = joinToString("") { "%02x".format(it.toInt() and 255) }
private fun JsonObject.text(name: String): String = get(name).asString

fun main(args: Array<String>) {
    val root = Path.of(args[0])
    val negative = JsonParser.parseString(Files.readString(root.resolve("tests/vectors/negative-batch-v1.json"))).asJsonObject
    val full = JsonParser.parseString(Files.readString(root.resolve("tests/vectors/full-wire-v1.json"))).asJsonObject
    val base = negative.getAsJsonObject("base")
    fun envelope(id: String): ByteArray = bytes(negative.getAsJsonArray("cases")
        .first { it.asJsonObject.text("id") == id }.asJsonObject.getAsJsonObject("input").text("envelope_cbor_hex"))
    val familyId = bytes(base.text("family_id_hex"))
    val relayId = bytes("03396219237f75a64f12aeb7f39723abf400b160c364980a765dac24aeba2464")
    val epochKey = bytes(base.text("epoch_key_hex"))
    val signer = ed25519PublicKey(bytes(base.text("recipient_sign_seed_hex")))
    val childId = bytes("0183f9d0000070008000000000000011")
    val minorInput = negative.getAsJsonArray("cases")
        .first { it.asJsonObject.text("id") == "CROSSMINORBYTE01" }.asJsonObject.getAsJsonObject("input")
    val sealedChild = sealOne(
        bytes(minorInput.text("header_cbor_hex")),
        bytes(minorInput.text("operation_hex")),
        epochKey,
        bytes(base.text("recipient_sign_seed_hex")),
    )
    check(sealedChild.hex() == minorInput.text("envelope_cbor_hex"))

    NativeFamily(familyId).use { family ->
        check(family.applyEnvelope(sealedChild, relayId, epochKey, signer, 1uL))
        check(family.fieldCbor(childId, 1uL).hex() == "6442616279")
        check(family.fieldCbor(childId, 500uL).hex() == "f4")
        check(family.lastCursor() == 1uL)
    }

    for (id in listOf("INERTBYTE01", "PRECREATEBYTE01", "WRONGSCOPEBYTE01", "PREFSBYTE01")) {
        NativeFamily(familyId).use { replay ->
            check(!replay.applyEnvelope(envelope(id), relayId, epochKey, signer, 1uL))
            check(replay.inertCount() == 1uL)
            check(replay.applyEnvelope(sealedChild, relayId, epochKey, signer, 2uL))
        }
    }

    NativeFamily(familyId).use { clean ->
        val wrongKey = epochKey.copyOf()
        wrongKey[0] = (wrongKey[0].toInt() xor 1).toByte()
        check(runCatching { clean.applyEnvelope(envelope("CROSSMINORBYTE01"), relayId, wrongKey, signer, 1uL) }.isFailure)
        val wrongSigner = signer.copyOf()
        wrongSigner[0] = (wrongSigner[0].toInt() xor 1).toByte()
        check(runCatching { clean.applyEnvelope(envelope("CROSSMINORBYTE01"), relayId, epochKey, wrongSigner, 1uL) }.isFailure)
        val wrongRelay = relayId.copyOf()
        wrongRelay[0] = (wrongRelay[0].toInt() xor 1).toByte()
        check(runCatching { clean.applyEnvelope(envelope("CROSSMINORBYTE01"), wrongRelay, epochKey, signer, 1uL) }.isFailure)
        check(clean.lastCursor() == 0uL)
    }
    val wrongFamilyId = familyId.copyOf()
    wrongFamilyId[0] = (wrongFamilyId[0].toInt() xor 1).toByte()
    NativeFamily(wrongFamilyId).use { wrongFamily ->
        check(runCatching { wrongFamily.applyEnvelope(envelope("CROSSMINORBYTE01"), relayId, epochKey, signer, 1uL) }.isFailure)
    }

    val fullCases = full.getAsJsonArray("cases")
    val genesis = fullCases.first { it.asJsonObject.text("id") == "GENESIS01" }.asJsonObject
    val fixed = fullCases.first { it.asJsonObject.text("id") == "BATCHBYTE01" }.asJsonObject
    val fixedSigner = ed25519PublicKey(bytes(genesis.getAsJsonObject("inputs").text("manager_sign_seed_hex")))
    val sealed = sealOne(
        bytes(fixed.getAsJsonObject("expect").text("header_cbor_hex")),
        bytes(fixed.getAsJsonObject("inputs").text("operation_cbor_hex")),
        bytes(fixed.getAsJsonObject("inputs").text("epoch_key_hex")),
        bytes(genesis.getAsJsonObject("inputs").text("manager_sign_seed_hex")),
    )
    check(sealed.hex() == fixed.getAsJsonObject("expect").text("envelope_cbor_hex"))
    NativeFamily(familyId).use { afterControl ->
        afterControl.advanceControl(1uL)
        check(afterControl.applyEnvelope(
            sealed,
            relayId,
            bytes(fixed.getAsJsonObject("inputs").text("epoch_key_hex")),
            fixedSigner,
            2uL,
        ))
        check(afterControl.fieldCbor(familyId, 1uL).hex() == "a0")
    }
    println("Kotlin fixed encrypted batch, minor field, inertness, and authentication: OK")
}
