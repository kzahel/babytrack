package org.babytrack.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.babytrack_core_ffi.FamilyRef
import uniffi.babytrack_core_ffi.ActivityWhen
import uniffi.babytrack_core_ffi.NativeLocalStore
import uniffi.babytrack_core_ffi.NativeSharedStore
import uniffi.babytrack_core_ffi.MedicationInput
import uniffi.babytrack_core_ffi.PumpInput
import uniffi.babytrack_core_ffi.BreastSegmentRow

/** Each method runs in a separate instrumentation invocation on one of two
 * emulators. The host transfers only the invitation fragment between them. */
@RunWith(AndroidJUnit4::class)
class TwoDeviceRelayTest {
    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext
    private val db get() = context.filesDir.resolve("two-device.db")
    private val origin = "http://localhost:8787"

    private fun family(): FamilyRef {
        val parts = context.filesDir.resolve("two-device-family.txt").readText().split(':')
        return FamilyRef(parts[0].decodeHex(), parts[1].decodeHex())
    }

    @Test fun managerCreate() {
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey required")
        val family = NativeLocalStore.open(db.absolutePath).use { local ->
            local.createFamily(System.currentTimeMillis()).also {
                local.addChildWithMetadata(it, "Shared child", 20_000L, 1u.toUByte(), System.currentTimeMillis())
            }
        }
        context.filesDir.resolve("two-device-family.txt").writeText(
            "${family.familyId.hex()}:${family.deviceId.hex()}"
        )
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            assertTrue(sharing.syncAndUpload(family, origin).ready)
            context.filesDir.resolve("two-device-link.txt").writeText(
                sharing.invite(family, origin, 1u.toUByte())
            )
        }
    }

    @Test fun recipientClaim() {
        val fragment = InstrumentationRegistry.getArguments().getString("fragment")
            ?: error("fragment required")
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            val claimed = sharing.claim(fragment)
            assertTrue(sharing.syncRecipient(claimed.family).awaitingGrant)
        }
    }

    @Test fun managerRespond() {
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            sharing.respondToClaim(family(), origin)
        }
    }

    @Test fun recipientProve() {
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            sharing.proveChallenge(sharing.recipientFamilies().single())
        }
    }

    @Test fun managerAdmit() {
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            sharing.admitProvedDevice(family(), origin)
        }
    }

    @Test fun recipientReadyAndUpload() {
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            val family = sharing.recipientFamilies().single()
            assertTrue(sharing.syncRecipient(family).ready)
            assertTrue(sharing.snapshot(family).children.any {
                it.name == "Shared child" && it.birthDay == 20_000L && it.sex == 1u.toUByte()
            })
            sharing.addChild(family, "Recipient child", System.currentTimeMillis())
            val end = System.currentTimeMillis()
            sharing.logSleep(family, sharing.snapshot(family).children.first().id,
                ActivityWhen(end - 30 * 60_000L, 0, end), end, 0)
            sharing.logNote(family, sharing.snapshot(family).children.first().id,
                "Care note marker 67", ActivityWhen(end, 0, end))
            sharing.logGrowth(family, sharing.snapshot(family).children.first().id,
                4_200, 540, ActivityWhen(end, 0, end))
            sharing.logTemperatureC(family, sharing.snapshot(family).children.first().id,
                "37.50", ActivityWhen(end, 0, end))
            sharing.logMedication(family, sharing.snapshot(family).children.first().id,
                MedicationInput("Test medicine marker 68", "2.5", "mL"), ActivityWhen(end, 0, end))
            sharing.logSolids(family, sharing.snapshot(family).children.first().id,
                listOf("Pear marker 69", "Oatmeal"), "two spoons", ActivityWhen(end, 0, end))
            sharing.logBreastFeed(family, sharing.snapshot(family).children.first().id,
                1u.toUByte(), ActivityWhen(end - 15 * 60_000L, 0, end), end)
            val breastStart = end - 16 * 60_000L
            sharing.logBreastFeedSegments(family, sharing.snapshot(family).children.first().id,
                listOf(
                    BreastSegmentRow(1u.toUByte(), breastStart, breastStart + 5 * 60_000L, 0, 0),
                    BreastSegmentRow(2u.toUByte(), breastStart + 5 * 60_000L, breastStart + 13 * 60_000L, 0, 0),
                    BreastSegmentRow(1u.toUByte(), breastStart + 13 * 60_000L, end, 0, 0),
                ), ActivityWhen(breastStart, 0, end))
            sharing.logPump(family, sharing.snapshot(family).children.first().id,
                PumpInput(20, 15, null), ActivityWhen(end - 10 * 60_000L, 0, end), end)
            sharing.startSleep(family, sharing.snapshot(family).children.first().id,
                ActivityWhen(end, 0, end))
            assertTrue(String(sharing.analysisCsv(family)).contains("Care note marker 67"))
            assertTrue(sharing.syncRecipientAndUpload(family).ready)
            assertEquals(0uL, sharing.snapshot(family).unsentCount)
        }
    }

    @Test fun managerVerifyAndUpload() {
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            val family = family()
            assertTrue(sharing.syncAndUpload(family, origin).ready)
            assertTrue(sharing.snapshot(family).children.any { it.name == "Recipient child" })
            assertTrue(sharing.snapshot(family).activities.any { it.kind == "sleep" && it.endUtcMs != null })
            assertTrue(sharing.snapshot(family).activities.any { it.note == "Care note marker 67" })
            assertTrue(String(sharing.analysisCsv(family)).contains("Care note marker 67"))
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "growth" && it.growthWeightG == 4_200L && it.growthLengthMm == 540L
            })
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "temperature" && it.temperatureC == "37.50"
            })
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "medication" && it.medicationName == "Test medicine marker 68" &&
                    it.medicationDoseAmount == "2.5" && it.medicationDoseUnit == "mL"
            })
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "feed.solids" && it.solidsFoods == listOf("Pear marker 69", "Oatmeal") &&
                    it.solidsAmount == "two spoons"
            })
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "feed.breast" && it.breastSide == 1u.toUByte() &&
                    it.endUtcMs != null && it.endUtcMs!! - it.startUtcMs == 15 * 60_000L
            })
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "feed.breast" && it.breastSegments?.map { segment -> segment.side } ==
                    listOf(1u.toUByte(), 2u.toUByte(), 1u.toUByte()) &&
                    it.endUtcMs != null && it.endUtcMs!! - it.startUtcMs == 16 * 60_000L
            })
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "pump" && it.pumpLeftMl == 20L && it.pumpRightMl == 15L &&
                    it.pumpTotalMl == null
            })
            val note = sharing.snapshot(family).activities.single { it.note == "Care note marker 67" }
            val otherChild = sharing.snapshot(family).children.single { !it.id.contentEquals(note.childId) }
            assertTrue(runCatching {
                sharing.deleteActivity(family, otherChild.id, note.id, System.currentTimeMillis())
            }.isFailure)
            assertTrue(runCatching {
                sharing.editNote(family, otherChild.id, note.id, "Wrong child", System.currentTimeMillis())
            }.isFailure)
            sharing.editNote(family, note.childId, note.id, "Corrected care note", System.currentTimeMillis())
            assertTrue(sharing.snapshot(family).activities.any {
                it.id.contentEquals(note.id) && it.note == "Corrected care note"
            })
            sharing.deleteActivity(family, note.childId, note.id, System.currentTimeMillis())
            assertTrue(sharing.snapshot(family).activities.none { it.id.contentEquals(note.id) })
            val editTime = System.currentTimeMillis()
            val retainedNote = sharing.logNote(
                family, note.childId, "Before sync correction", ActivityWhen(editTime, 0, editTime)
            )
            sharing.editNote(
                family, note.childId, retainedNote, "After sync correction", System.currentTimeMillis()
            )
            val bottleTime = System.currentTimeMillis()
            val bottle = sharing.logBottleMl(
                family, note.childId, 90, ActivityWhen(bottleTime, 0, bottleTime)
            )
            assertTrue(runCatching {
                sharing.editBottleMl(family, otherChild.id, bottle, 120, System.currentTimeMillis())
            }.isFailure)
            sharing.editBottleMl(family, note.childId, bottle, 120, System.currentTimeMillis())
            val diaperTime = System.currentTimeMillis()
            val diaper = sharing.logDiaper(
                family, note.childId, 1u.toUByte(), ActivityWhen(diaperTime, 0, diaperTime)
            )
            assertTrue(runCatching {
                sharing.editDiaperKind(family, otherChild.id, diaper, 2u.toUByte(), System.currentTimeMillis())
            }.isFailure)
            sharing.editDiaperKind(family, note.childId, diaper, 2u.toUByte(), System.currentTimeMillis())
            assertTrue(sharing.snapshot(family).activities.any {
                it.id.contentEquals(diaper) && it.diaperKind == 2u.toUByte()
            })
            val running = sharing.snapshot(family).activities.single {
                it.kind == "sleep" && it.endUtcMs == null
            }
            val stoppedAt = System.currentTimeMillis()
            sharing.stopSleep(family, running.childId, running.id, stoppedAt, 0)
            sharing.addChild(family, "Manager child", System.currentTimeMillis())
            assertTrue(sharing.syncAndUpload(family, origin).ready)
        }
    }

    @Test fun recipientVerifyAndSaveOffline() {
        val family = ShareCoordinator(context, db.absolutePath).use { sharing ->
            val family = sharing.recipientFamilies().single()
            assertTrue(sharing.syncRecipientAndUpload(family).ready)
            assertTrue(sharing.snapshot(family).children.any { it.name == "Manager child" })
            assertTrue(sharing.snapshot(family).activities.none { it.note == "Care note marker 67" })
            assertTrue(sharing.snapshot(family).activities.any { it.note == "After sync correction" })
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "feed.bottle" && it.bottleMl == 120L
            })
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "diaper" && it.diaperKind == 2u.toUByte()
            })
            assertTrue(sharing.snapshot(family).activities.count { it.kind == "sleep" && it.endUtcMs != null } == 2)
            sharing.addChild(family, "Pending at removal", System.currentTimeMillis())
            assertEquals(1uL, sharing.snapshot(family).unsentCount)
            family
        }
        val wrapping = DeviceWrappingKey(context).loadOrCreate()
        try {
            NativeSharedStore.open(db.absolutePath).use { core ->
                assertTrue(core.prepareSharedUpload(family, wrapping) != null)
            }
        } finally {
            wrapping.fill(0)
        }
    }

    @Test fun managerRemove() {
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            val family = family()
            val target = sharing.snapshot(family).devices.single {
                !it.deviceId.contentEquals(family.deviceId)
            }
            sharing.removeDevice(family, origin, target.deviceId)
            assertEquals(1, sharing.snapshot(family).devices.size)
        }
    }

    @Test fun recipientRemovedAndCopied() {
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            val family = sharing.recipientFamilies().single()
            val removed = sharing.advanceRecipient(family)
            assertTrue(removed.removed)
            assertEquals(1u.toUByte(), removed.pendingResult)
            val copy = removed.privateCopy ?: error("Pending edit needs a private copy")
            assertTrue(!sharing.isShared(copy))
            val repeated = sharing.advanceRecipient(family).privateCopy
                ?: error("Repeated removal needs the same private copy")
            assertTrue(copy.familyId.contentEquals(repeated.familyId))
            assertTrue(copy.deviceId.contentEquals(repeated.deviceId))
            NativeLocalStore.open(db.absolutePath).use { local ->
                assertTrue(local.children(copy).any { it.name == "Pending at removal" })
            }
            assertTrue(runCatching { sharing.syncRecipientAndUpload(family) }.isFailure)
        }
    }
}

private fun ByteArray.hex(): String = joinToString("") { "%02x".format(it.toInt() and 255) }
private fun String.decodeHex(): ByteArray = chunked(2).map { it.toInt(16).toByte() }.toByteArray()
