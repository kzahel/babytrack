package org.babytrack.app

import android.content.Context
import uniffi.babytrack_core_ffi.FamilyRef
import uniffi.babytrack_core_ffi.NativeSharedStore
import uniffi.babytrack_core_ffi.PreparedJoinRow
import uniffi.babytrack_core_ffi.previewInvitation
import uniffi.babytrack_core_ffi.validateRelayOrigin

/** Platform transport for the Rust sharing preparation and confirmation API. */
internal class ShareCoordinator(context: Context, databasePath: String) : AutoCloseable {
    private val keys = DeviceWrappingKey(context)
    private val core = NativeSharedStore.open(databasePath)

    fun promote(family: FamilyRef, origin: String, publicKeyHex: String): ULong {
        validateRelayOrigin(origin)
        val relay = RelayTransport(origin)
        val publicKey = parsePublicKey(publicKeyHex)
        val wrapping = keys.loadOrCreate()
        try {
            val prepared = core.prepareShare(family, publicKey, wrapping)
            val prefix = "/v1/families/${family.familyId.hex()}"
            for (objectRow in prepared.objects) {
                relay.post("$prefix/objects/${objectRow.objectId.hex()}", objectRow.body)
            }
            val response = relay.post("$prefix/control", prepared.candidateBytes)
            return core.confirmShare(family, wrapping, response)
        } finally {
            wrapping.fill(0)
        }
    }

    fun invite(family: FamilyRef, origin: String, role: UByte): String {
        validateRelayOrigin(origin)
        val relay = RelayTransport(origin)
        val wrapping = keys.loadOrCreate()
        try {
            val prepared = core.prepareInvite(family, wrapping, role)
            val prefix = "/v1/families/${family.familyId.hex()}"
            relay.post("$prefix/objects/${prepared.`object`.objectId.hex()}", prepared.`object`.body)
            val response = relay.post("$prefix/control", prepared.candidateBytes)
            return core.confirmInvite(family, wrapping, response, origin)
        } finally {
            wrapping.fill(0)
        }
    }

    fun claim(fragment: String): PreparedJoinRow {
        val preview = previewInvitation(fragment)
        val relay = RelayTransport(preview.relayOrigin)
        val wrapping = keys.loadOrCreate()
        try {
            val prepared = core.resumeJoin(fragment, wrapping) ?: run {
                val page = relay.get(preview.controlPath, preview.readAuth)
                core.prepareJoin(fragment, page, wrapping)
            }
            val path = "/v1/families/${prepared.family.familyId.hex()}/control"
            val response = relay.post(path, prepared.candidateBytes)
            core.confirmJoinClaim(prepared.family, wrapping, response)
            return prepared
        } finally {
            wrapping.fill(0)
        }
    }

    fun respondToClaim(family: FamilyRef, origin: String) {
        validateRelayOrigin(origin)
        val relay = RelayTransport(origin)
        val wrapping = keys.loadOrCreate()
        try {
            val read = core.managerControlRead(family, wrapping)
            val page = relay.get(read.path, read.auth)
            val prepared = core.prepareFirstChallenge(family, wrapping, read, page)
            val prefix = "/v1/families/${family.familyId.hex()}"
            for (objectRow in prepared.objects) {
                relay.post("$prefix/objects/${objectRow.objectId.hex()}", objectRow.body)
            }
            val response = relay.post("$prefix/control", prepared.candidateBytes)
            core.confirmFirstChallenge(family, wrapping, response)
        } finally {
            wrapping.fill(0)
        }
    }

    override fun close() = core.close()
}

private fun parsePublicKey(text: String): ByteArray {
    val hex = text.trim().lowercase()
    require(hex.length == 64 && hex.all { it in '0'..'9' || it in 'a'..'f' }) {
        "Relay public key must be 32 hexadecimal bytes"
    }
    return hex.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
}

private fun ByteArray.hex(): String = joinToString("") { "%02x".format(it.toInt() and 255) }
