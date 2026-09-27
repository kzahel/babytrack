package org.babytrack.app

import android.content.Context
import uniffi.babytrack_core_ffi.FamilyRef
import uniffi.babytrack_core_ffi.ActivityWhen
import uniffi.babytrack_core_ffi.BackupFileRow
import uniffi.babytrack_core_ffi.NativeSharedStore
import uniffi.babytrack_core_ffi.PreparedJoinRow
import uniffi.babytrack_core_ffi.RecipientSyncRow
import uniffi.babytrack_core_ffi.RemovedDeviceRow
import uniffi.babytrack_core_ffi.RelayReadTransport
import uniffi.babytrack_core_ffi.SharedSnapshotRow
import uniffi.babytrack_core_ffi.SharedSyncRow
import uniffi.babytrack_core_ffi.previewInvitation
import uniffi.babytrack_core_ffi.invitationControlRead
import uniffi.babytrack_core_ffi.invitationStatusRead
import uniffi.babytrack_core_ffi.verifyInvitationStatus
import uniffi.babytrack_core_ffi.controlPageProgress
import uniffi.babytrack_core_ffi.validateRelayOrigin

internal class SharedUploadBlocked : IllegalStateException("Signed relay rejection retained the saved batch")

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
            val saved = core.resumeJoin(fragment, wrapping)
            val pages = joinControlPages(relay, preview.familyId, wrapping, fragment, saved?.family)
            val prepared = core.prepareJoinPages(fragment, pages, wrapping)
            val path = "/v1/families/${prepared.family.familyId.hex()}/control"
            val response = relay.post(path, prepared.candidateBytes)
            core.confirmJoinClaim(prepared.family, wrapping, response)
            return prepared
        } finally {
            wrapping.fill(0)
        }
    }

    fun retryClaim(family: FamilyRef): PreparedJoinRow {
        val relay = RelayTransport(recipientOrigin(family))
        return withWrapping { wrapping ->
            val pages = joinControlPages(relay, family.familyId, wrapping, null, family)
            val prepared = core.refreshJoinPages(family, wrapping, pages)
            val response = relay.post("/v1/families/${family.familyId.hex()}/control", prepared.candidateBytes)
            core.confirmJoinClaim(family, wrapping, response)
            prepared
        }
    }

    private fun joinControlPages(
        relay: RelayTransport,
        familyId: ByteArray,
        wrapping: ByteArray,
        fragment: String?,
        saved: FamilyRef?,
    ): List<ByteArray> {
        val pages = mutableListOf<ByteArray>()
        var after = 0uL
        var totalBytes = 0L
        while (true) {
            check(pages.size < 64) { "Invitation control history exceeds the current page limit" }
            val read = if (fragment != null) invitationControlRead(fragment, after)
                else core.savedJoinControlRead(saved ?: error("Saved join missing"), wrapping, after, false)
            val page = try {
                relay.get(read.path, read.auth)
            } catch (failure: IllegalStateException) {
                try {
                    if (saved == null) throw failure
                    val pending = core.savedJoinControlRead(saved, wrapping, after, true)
                    relay.get(pending.path, pending.auth)
                } catch (pendingFailure: IllegalStateException) {
                    val status = try {
                        val statusRead = if (fragment != null) invitationStatusRead(fragment)
                            else core.savedInvitationStatusRead(saved ?: throw pendingFailure, wrapping)
                        val response = relay.get(statusRead.path, statusRead.auth)
                        if (fragment != null) verifyInvitationStatus(fragment, response)
                            else core.verifySavedInvitationStatus(saved ?: throw pendingFailure, wrapping, response)
                    } catch (_: Exception) {
                        null
                    }
                    val terminal = when (status?.reason) {
                        2u.toUByte() -> "Invitation already claimed"
                        3u.toUByte() -> "Invitation canceled"
                        4u.toUByte() -> "Invitation expired"
                        5u.toUByte() -> "Invitation issuer no longer has access"
                        else -> null
                    }
                    if (terminal != null) throw IllegalStateException(terminal)
                    throw IllegalStateException("Invitation status cannot be verified yet", pendingFailure)
                }
            }
            totalBytes += page.size
            check(totalBytes <= 16L * 1024 * 1024) { "Invitation control history exceeds the current size limit" }
            val progress = controlPageProgress(familyId, page, after)
            pages.add(page)
            if (!progress.hasMore) break
            after = progress.nextAfter
        }
        return pages
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

    fun proveChallenge(fragment: String) {
        val family = withWrapping { wrapping ->
            core.resumeJoin(fragment, wrapping)?.family ?: error("No durable recipient claim")
        }
        proveChallenge(family)
    }

    fun proveChallenge(family: FamilyRef) {
        val relay = RelayTransport(recipientOrigin(family))
        val wrapping = keys.loadOrCreate()
        try {
            val candidate = core.savedFirstProof(family, wrapping) ?: run {
                val read = core.recipientControlRead(family, wrapping)
                val page = relay.get(read.path, read.auth)
                val challenge = core.recipientChallengeRead(family, wrapping, read, page)
                val objectResponse = relay.get(challenge.path, challenge.auth)
                core.prepareFirstProof(family, wrapping, challenge, objectResponse)
            }
            val response = relay.post("/v1/families/${family.familyId.hex()}/control", candidate)
            core.confirmFirstProof(family, wrapping, response)
        } finally {
            wrapping.fill(0)
        }
    }

    fun admitProvedDevice(family: FamilyRef, origin: String) {
        validateRelayOrigin(origin)
        val relay = RelayTransport(origin)
        val wrapping = keys.loadOrCreate()
        try {
            val read = core.managerControlRead(family, wrapping)
            val page = relay.get(read.path, read.auth)
            val prepared = core.prepareFirstAdmission(family, wrapping, read, page)
            val prefix = "/v1/families/${family.familyId.hex()}"
            for (objectRow in prepared.objects) {
                relay.post("$prefix/objects/${objectRow.objectId.hex()}", objectRow.body)
            }
            val response = relay.post("$prefix/control", prepared.candidateBytes)
            core.confirmFirstAdmission(family, wrapping, response)
        } finally {
            wrapping.fill(0)
        }
    }

    fun removeDevice(family: FamilyRef, origin: String, targetDeviceId: ByteArray): SharedSnapshotRow {
        validateRelayOrigin(origin)
        check(syncAndUpload(family, origin).ready) { "Shared history is not ready" }
        val relay = RelayTransport(origin)
        withWrapping { wrapping ->
            val prepared = core.prepareFirstRemoval(family, wrapping, targetDeviceId)
            val prefix = "/v1/families/${family.familyId.hex()}"
            for (objectRow in prepared.objects) {
                relay.post("$prefix/objects/${objectRow.objectId.hex()}", objectRow.body)
            }
            val response = relay.post("$prefix/control", prepared.candidateBytes)
            core.confirmFirstRemoval(family, wrapping, response)
        }
        return snapshot(family)
    }

    fun syncRecipient(fragment: String): RecipientSyncRow {
        val family = withWrapping { wrapping ->
            core.resumeJoin(fragment, wrapping)?.family ?: error("No durable recipient claim")
        }
        return syncRecipient(family)
    }

    fun syncRecipient(family: FamilyRef): RecipientSyncRow {
        val relay = RelayTransport(recipientOrigin(family))
        val wrapping = keys.loadOrCreate()
        try {
            val removed = core.checkRecipientRemoval(family, wrapping, System.currentTimeMillis(), object : RelayReadTransport {
                override fun get(path: String, auth: ByteArray): ByteArray = relay.get(path, auth)
            })
            if (removed != null) return removedProgress(removed)
            return core.syncRecipient(family, wrapping, object : RelayReadTransport {
                override fun get(path: String, auth: ByteArray): ByteArray = relay.get(path, auth)
            })
        } finally {
            wrapping.fill(0)
        }
    }

    fun snapshot(family: FamilyRef): SharedSnapshotRow {
        val wrapping = keys.loadOrCreate()
        try {
            return core.sharedSnapshot(family, wrapping)
        } finally {
            wrapping.fill(0)
        }
    }

    fun backupFile(family: FamilyRef, nowMs: Long, password: String?, availableMemory: ULong): BackupFileRow =
        withWrapping { wrapping -> core.sharedBackupFile(family, wrapping, nowMs, password, availableMemory) }

    fun analysisCsv(family: FamilyRef): ByteArray =
        withWrapping { wrapping -> core.sharedAnalysisCsv(family, wrapping) }

    fun privateCopy(family: FamilyRef, nowMs: Long): FamilyRef =
        withWrapping { wrapping -> core.privateCopyShared(family, wrapping, nowMs) }

    fun isShared(family: FamilyRef): Boolean = core.isShared(family)

    fun snapshotForFragment(fragment: String): SharedSnapshotRow {
        val wrapping = keys.loadOrCreate()
        try {
            val family = core.resumeJoin(fragment, wrapping)?.family
                ?: error("No durable recipient claim")
            return core.sharedSnapshot(family, wrapping)
        } finally {
            wrapping.fill(0)
        }
    }

    fun recipientFamilies(): List<FamilyRef> = core.recipientFamilies()

    fun recipientOrigin(family: FamilyRef): String = withWrapping { wrapping ->
        core.recipientRelayOrigin(family, wrapping)
    }

    fun addChild(family: FamilyRef, name: String, nowMs: Long): ByteArray = withWrapping { wrapping ->
        core.addSharedChild(family, wrapping, name, nowMs)
    }

    fun addChildWithMetadata(
        family: FamilyRef, name: String, birthDay: Long?, sex: UByte?, nowMs: Long,
    ): ByteArray = withWrapping { wrapping ->
        core.addSharedChildWithMetadata(family, wrapping, name, birthDay, sex, nowMs)
    }

    fun logDiaper(family: FamilyRef, childId: ByteArray, kind: UByte, time: ActivityWhen): ByteArray =
        withWrapping { wrapping -> core.logSharedDiaper(family, wrapping, childId, kind, time) }

    fun logBottleMl(family: FamilyRef, childId: ByteArray, amountMl: Long, time: ActivityWhen): ByteArray =
        withWrapping { wrapping -> core.logSharedBottleMl(family, wrapping, childId, amountMl, 2u.toUByte(), time) }

    fun logBreastFeed(family: FamilyRef, childId: ByteArray, side: UByte, time: ActivityWhen, endUtcMs: Long): ByteArray =
        withWrapping { wrapping -> core.logSharedBreastFeed(family, wrapping, childId, side, time, endUtcMs) }

    fun logBreastFeedSegments(family: FamilyRef, childId: ByteArray,
                              segments: List<uniffi.babytrack_core_ffi.BreastSegmentRow>, time: ActivityWhen): ByteArray =
        withWrapping { wrapping -> core.logSharedBreastFeedSegments(family, wrapping, childId, segments, time) }

    fun logPump(family: FamilyRef, childId: ByteArray, input: uniffi.babytrack_core_ffi.PumpInput, time: ActivityWhen, endUtcMs: Long): ByteArray =
        withWrapping { wrapping -> core.logSharedPump(family, wrapping, childId, input, time, endUtcMs) }

    fun logSolids(family: FamilyRef, childId: ByteArray, foods: List<String>, amount: String, time: ActivityWhen): ByteArray =
        withWrapping { wrapping -> core.logSharedSolids(family, wrapping, childId, foods, amount, time) }

    fun logSleep(family: FamilyRef, childId: ByteArray, time: ActivityWhen, endUtcMs: Long, endOffsetMinutes: Short): ByteArray =
        withWrapping { wrapping -> core.logSharedSleep(family, wrapping, childId, time, endUtcMs, endOffsetMinutes) }

    fun startSleep(family: FamilyRef, childId: ByteArray, time: ActivityWhen): ByteArray =
        withWrapping { wrapping -> core.startSharedSleep(family, wrapping, childId, time) }

    fun stopSleep(family: FamilyRef, childId: ByteArray, activityId: ByteArray, endUtcMs: Long, endOffsetMinutes: Short): Unit =
        withWrapping { wrapping -> core.stopSharedSleep(family, wrapping, childId, activityId,
            ActivityWhen(endUtcMs, endOffsetMinutes, endUtcMs)) }

    fun deleteActivity(family: FamilyRef, childId: ByteArray, activityId: ByteArray, savedAtMs: Long): Unit =
        withWrapping { wrapping -> core.deleteSharedActivity(family, wrapping, childId, activityId, savedAtMs) }

    fun editNote(family: FamilyRef, childId: ByteArray, activityId: ByteArray, note: String, savedAtMs: Long): Unit =
        withWrapping { wrapping -> core.editSharedNote(family, wrapping, childId, activityId, note, savedAtMs) }

    fun editBottleMl(family: FamilyRef, childId: ByteArray, activityId: ByteArray, amountMl: Long, savedAtMs: Long): Unit =
        withWrapping { wrapping -> core.editSharedBottleMl(family, wrapping, childId, activityId, amountMl, savedAtMs) }

    fun editDiaperKind(family: FamilyRef, childId: ByteArray, activityId: ByteArray, kind: UByte, savedAtMs: Long): Unit =
        withWrapping { wrapping -> core.editSharedDiaperKind(family, wrapping, childId, activityId, kind, savedAtMs) }

    fun editSolids(family: FamilyRef, childId: ByteArray, activityId: ByteArray,
                   foods: List<String>, amount: String, savedAtMs: Long): Unit =
        withWrapping { wrapping -> core.editSharedSolids(family, wrapping, childId, activityId, foods, amount, savedAtMs) }

    fun logNote(family: FamilyRef, childId: ByteArray, note: String, time: ActivityWhen): ByteArray =
        withWrapping { wrapping -> core.logSharedNote(family, wrapping, childId, note, time) }

    fun logGrowth(family: FamilyRef, childId: ByteArray, weightG: Long?, lengthMm: Long?, time: ActivityWhen): ByteArray =
        withWrapping { wrapping -> core.logSharedGrowth(family, wrapping, childId, weightG, lengthMm, time) }

    fun logTemperatureC(family: FamilyRef, childId: ByteArray, enteredC: String, time: ActivityWhen): ByteArray =
        withWrapping { wrapping -> core.logSharedTemperatureC(family, wrapping, childId, enteredC, time) }

    fun logMedication(family: FamilyRef, childId: ByteArray, input: uniffi.babytrack_core_ffi.MedicationInput, time: ActivityWhen): ByteArray =
        withWrapping { wrapping -> core.logSharedMedication(family, wrapping, childId, input, time) }

    fun syncAndUpload(family: FamilyRef, origin: String): SharedSyncRow {
        validateRelayOrigin(origin)
        val relay = RelayTransport(origin)
        return withWrapping { wrapping ->
            val reads = object : RelayReadTransport {
                override fun get(path: String, auth: ByteArray): ByteArray = relay.get(path, auth)
            }
            var progress = core.syncShared(family, wrapping, reads)
            repeat(16) {
                if (!progress.ready) return@withWrapping progress
                when (core.resolvePendingBatchResult(family, wrapping, reads).toInt()) {
                    2 -> return@withWrapping progress
                    4 -> throw SharedUploadBlocked()
                }
                val bytes = core.prepareSharedUpload(family, wrapping) ?: return@withWrapping progress
                relay.post("/v1/families/${family.familyId.hex()}/batches", bytes, allowBatchConflict = true)
                progress = core.syncShared(family, wrapping, reads)
            }
            progress
        }
    }

    fun syncRecipientAndUpload(fragment: String): SharedSyncRow {
        val family = snapshotForFragment(fragment).family
        return syncRecipientAndUpload(family)
    }

    fun syncRecipientAndUpload(family: FamilyRef): SharedSyncRow {
        check(!syncRecipient(family).removed) { "Verified device removal; shared uploads stopped" }
        return syncAndUpload(family, recipientOrigin(family))
    }

    fun advanceManager(family: FamilyRef, origin: String): SharedSyncRow {
        val progress = syncAndUpload(family, origin)
        if (!progress.ready) return progress
        when (withWrapping { wrapping -> core.managerFirstJoinAction(family, wrapping) }.toInt()) {
            1 -> respondToClaim(family, origin)
            2 -> admitProvedDevice(family, origin)
        }
        return progress
    }

    fun advanceRecipient(family: FamilyRef): RecipientSyncRow {
        if (withWrapping { wrapping -> core.recipientFirstJoinAction(family, wrapping) }.toInt() == 2) {
            retryClaim(family)
        }
        var progress = syncRecipient(family)
        if (withWrapping { wrapping -> core.recipientFirstJoinAction(family, wrapping) }.toInt() == 1) {
            proveChallenge(family)
            progress = syncRecipient(family)
        }
        if (progress.ready) syncRecipientAndUpload(family)
        return progress
    }

    private inline fun <T> withWrapping(action: (ByteArray) -> T): T {
        val wrapping = keys.loadOrCreate()
        try {
            return action(wrapping)
        } finally {
            wrapping.fill(0)
        }
    }

    override fun close() = core.close()
}

private fun removedProgress(removed: RemovedDeviceRow): RecipientSyncRow = RecipientSyncRow(
    verifiedCursor = removed.verifiedCursor,
    pendingControlCursor = removed.verifiedCursor,
    awaitingGrant = false,
    noMoreVisible = false,
    remainingObjects = false,
    ready = false,
    childCount = 0uL,
    removed = true,
    privateCopy = removed.privateCopy,
    pendingResult = removed.pendingResult,
)

private fun parsePublicKey(text: String): ByteArray {
    val hex = text.trim().lowercase()
    require(hex.length == 64 && hex.all { it in '0'..'9' || it in 'a'..'f' }) {
        "Relay public key must be 32 hexadecimal bytes"
    }
    return hex.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
}

private fun ByteArray.hex(): String = joinToString("") { "%02x".format(it.toInt() and 255) }
