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

    @Test fun managerLaterInvite() {
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            context.filesDir.resolve("two-device-later-link.txt").writeText(
                sharing.invite(family(), origin, 2u.toUByte())
            )
        }
    }

    @Test fun recipientLaterReadyAndUpload() {
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            val family = sharing.recipientFamilies().single()
            assertTrue(sharing.syncRecipient(family).ready)
            val snapshot = sharing.snapshot(family)
            assertTrue(snapshot.children.any { it.name == "Renamed shared child" })
            assertTrue(snapshot.children.any { it.name == "Recipient child" })
            sharing.logNote(family, snapshot.children.first().id,
                "Later recipient marker 70", ActivityWhen(System.currentTimeMillis(), 0, System.currentTimeMillis()))
            assertTrue(sharing.syncRecipientAndUpload(family).ready)
        }
    }

    @Test fun managerReadsLaterRecipient() {
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            assertTrue(sharing.syncAndUpload(family(), origin).ready)
            assertTrue(sharing.snapshot(family()).activities.any { it.note == "Later recipient marker 70" })
        }
    }

    @Test fun recipientClaim() {
        val fragment = InstrumentationRegistry.getArguments().getString("fragment")
            ?: error("fragment required")
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            val claimed = sharing.claim(fragment)
            val progress = sharing.syncRecipient(claimed.family)
            assertTrue(progress.awaitingGrant)
            assertEquals(2u.toUByte(), progress.joinPhase)
        }
    }

    @Test fun managerRespond() {
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            sharing.respondToClaim(family(), origin)
        }
    }

    @Test fun managerAdvanceJoin() {
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            assertTrue(sharing.advanceManager(family(), origin).ready)
        }
    }

    @Test fun recipientProve() {
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            sharing.proveChallenge(sharing.recipientFamilies().single())
        }
    }

    @Test fun recipientAdvanceJoin() {
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            val family = sharing.recipientFamilies().single()
            val progress = sharing.advanceRecipient(family)
            assertTrue(progress.awaitingGrant || progress.ready)
            assertTrue(progress.joinPhase == 4u.toUByte() || progress.joinPhase == 6u.toUByte())
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
            val originalChild = sharing.snapshot(family).children.single { it.name == "Shared child" }
            sharing.renameChild(family, originalChild.id, "Renamed shared child", System.currentTimeMillis())
            sharing.editChildMetadata(family, originalChild.id, 20_001L, 2u.toUByte(), System.currentTimeMillis())
            sharing.addChild(family, "Recipient child", System.currentTimeMillis())
            val end = System.currentTimeMillis()
            sharing.logSleepWithPlace(family, sharing.snapshot(family).children.first().id,
                ActivityWhen(end - 30 * 60_000L, 0, end), end, 0, 1u.toUByte())
            sharing.logNote(family, sharing.snapshot(family).children.first().id,
                "Care note marker 67", ActivityWhen(end, 0, end))
            sharing.logGrowthMeasurements(family, sharing.snapshot(family).children.first().id,
                4_200, 540, 350, ActivityWhen(end, 0, end))
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
            sharing.startSleepWithPlace(family, sharing.snapshot(family).children.first().id,
                ActivityWhen(end, 0, end), 3u.toUByte())
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
            assertTrue(sharing.snapshot(family).children.any {
                it.name == "Renamed shared child" && it.birthDay == 20_001L && it.sex == 2u.toUByte()
            })
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "sleep" && it.endUtcMs != null && it.sleepPlace == 1u.toUByte()
            })
            val completedSleep = sharing.snapshot(family).activities.single {
                it.kind == "sleep" && it.endUtcMs != null
            }
            val correctedEnd = completedSleep.startUtcMs + 20 * 60_000L
            sharing.editSleepEnd(family, completedSleep.childId, completedSleep.id,
                correctedEnd, 0, System.currentTimeMillis())
            sharing.editSleepPlace(family, completedSleep.childId, completedSleep.id,
                2u.toUByte(), System.currentTimeMillis())
            assertTrue(sharing.snapshot(family).activities.any {
                it.id.contentEquals(completedSleep.id) && it.startUtcMs == completedSleep.startUtcMs &&
                    it.endUtcMs == correctedEnd && it.sleepPlace == 2u.toUByte()
            })
            assertTrue(sharing.snapshot(family).activities.any { it.note == "Care note marker 67" })
            assertTrue(String(sharing.analysisCsv(family)).contains("Care note marker 67"))
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "growth" && it.growthWeightG == 4_200L && it.growthLengthMm == 540L &&
                    it.growthHeadMm == 350L
            })
            val growth = sharing.snapshot(family).activities.single { it.kind == "growth" }
            sharing.editGrowthMeasurements(family, growth.childId, growth.id, 4_300L, null, 355L, System.currentTimeMillis())
            assertTrue(sharing.snapshot(family).activities.any {
                it.id.contentEquals(growth.id) && it.growthWeightG == 4_300L &&
                    it.growthLengthMm == 540L && it.growthHeadMm == 355L &&
                    it.startUtcMs == growth.startUtcMs
            })
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "temperature" && it.temperatureC == "37.50"
            })
            val temperature = sharing.snapshot(family).activities.single { it.kind == "temperature" }
            sharing.editTemperatureEntered(family, temperature.childId, temperature.id,
                "99", 31u.toUByte(), System.currentTimeMillis())
            assertTrue(sharing.snapshot(family).activities.any {
                it.id.contentEquals(temperature.id) && it.temperatureC == "37.22" &&
                    it.temperatureEntered == "99" && it.temperatureUnit == 31u.toUByte() &&
                    it.startUtcMs == temperature.startUtcMs
            })
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "medication" && it.medicationName == "Test medicine marker 68" &&
                    it.medicationDoseAmount == "2.5" && it.medicationDoseUnit == "mL"
            })
            val medication = sharing.snapshot(family).activities.single { it.kind == "medication" }
            val wrongMedicationChild = sharing.snapshot(family).children.single {
                !it.id.contentEquals(medication.childId)
            }
            assertTrue(runCatching {
                sharing.editMedication(family, wrongMedicationChild.id, medication.id,
                    MedicationInput("Corrected medicine", "3", "mL"), System.currentTimeMillis())
            }.isFailure)
            sharing.editMedication(family, medication.childId, medication.id,
                MedicationInput("Corrected medicine", "3", "mL"), System.currentTimeMillis())
            assertTrue(sharing.snapshot(family).activities.any {
                it.id.contentEquals(medication.id) && it.medicationName == "Corrected medicine" &&
                    it.medicationDoseAmount == "3" && it.medicationDoseUnit == "mL" &&
                    it.startUtcMs == medication.startUtcMs
            })
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "feed.solids" && it.solidsFoods == listOf("Pear marker 69", "Oatmeal") &&
                    it.solidsAmount == "two spoons"
            })
            val solids = sharing.snapshot(family).activities.single { it.kind == "feed.solids" }
            val wrongSolidsChild = sharing.snapshot(family).children.single {
                !it.id.contentEquals(solids.childId)
            }
            assertTrue(runCatching {
                sharing.editSolids(family, wrongSolidsChild.id, solids.id,
                    listOf("Apple"), "half bowl", System.currentTimeMillis())
            }.isFailure)
            sharing.editSolids(family, solids.childId, solids.id,
                listOf("Apple", "Rice"), "half bowl", System.currentTimeMillis())
            assertTrue(sharing.snapshot(family).activities.any {
                it.id.contentEquals(solids.id) && it.solidsFoods == listOf("Apple", "Rice") &&
                    it.solidsAmount == "half bowl" && it.startUtcMs == solids.startUtcMs
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
            val breast = sharing.snapshot(family).activities.single {
                it.kind == "feed.breast" && it.breastSegments?.size == 3
            }
            val correctedSegments = listOf(
                BreastSegmentRow(2u.toUByte(), breast.startUtcMs, breast.startUtcMs + 6 * 60_000L, 0, 0),
                BreastSegmentRow(1u.toUByte(), breast.startUtcMs + 6 * 60_000L,
                    breast.startUtcMs + 13 * 60_000L, 0, 0),
                BreastSegmentRow(2u.toUByte(), breast.startUtcMs + 13 * 60_000L,
                    breast.startUtcMs + 16 * 60_000L, 0, 0),
            )
            sharing.editBreastFeedSegments(family, breast.childId, breast.id,
                correctedSegments, System.currentTimeMillis())
            assertTrue(sharing.snapshot(family).activities.any {
                it.id.contentEquals(breast.id) && it.breastSegments?.map { segment -> segment.side } ==
                    listOf(2u.toUByte(), 1u.toUByte(), 2u.toUByte())
            })
            val pump = sharing.snapshot(family).activities.single {
                it.kind == "pump" && it.pumpLeftMl == 20L && it.pumpRightMl == 15L &&
                    it.pumpTotalMl == null
            }
            sharing.editPumpAmounts(family, pump.childId, pump.id,
                PumpInput(null, null, 40), System.currentTimeMillis())
            assertTrue(sharing.snapshot(family).activities.any {
                it.id.contentEquals(pump.id) && it.pumpLeftMl == null && it.pumpRightMl == null &&
                    it.pumpTotalMl == 40L && it.startUtcMs == pump.startUtcMs && it.endUtcMs == pump.endUtcMs
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
                family, note.childId, 90, 1u.toUByte(), ActivityWhen(bottleTime, 0, bottleTime)
            )
            assertTrue(runCatching {
                sharing.editBottleMl(family, otherChild.id, bottle, 120, System.currentTimeMillis())
            }.isFailure)
            sharing.editBottleEntered(
                family, note.childId, bottle, "4.5", 2u.toUByte(), 3u.toUByte(), System.currentTimeMillis()
            )
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
            assertEquals(3u.toUByte(), running.sleepPlace)
            val stoppedAt = System.currentTimeMillis()
            sharing.stopSleep(family, running.childId, running.id, stoppedAt, 0)
            sharing.addChild(family, "Manager child", System.currentTimeMillis())
            val uploaded = sharing.syncAndUpload(family, origin)
            assertTrue(uploaded.ready)
            assertEquals("outbox ${uploaded.outboxState}, cursor ${uploaded.verifiedCursor}, inert ${uploaded.inertCount}",
                0uL, sharing.snapshot(family).unsentCount)
        }
    }

    @Test fun recipientVerifyAndSaveOffline() {
        val family = ShareCoordinator(context, db.absolutePath).use { sharing ->
            val family = sharing.recipientFamilies().single()
            assertTrue(sharing.syncRecipientAndUpload(family).ready)
            val synced = sharing.snapshot(family)
            val children = synced.children
            assertTrue("Recipient children: ${children.map { it.name }}; inert: ${synced.inertCount}; cursor: ${synced.verifiedCursor}",
                children.any { it.name == "Manager child" })
            assertTrue(sharing.snapshot(family).activities.none { it.note == "Care note marker 67" })
            assertTrue(sharing.snapshot(family).activities.any { it.note == "After sync correction" })
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "feed.bottle" && it.bottleMl == 133L &&
                    it.bottleEntered == "4.5" && it.bottleUnit == 2u.toUByte() &&
                    it.bottleContent == 3u.toUByte()
            })
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "sleep" && it.endUtcMs != null && it.sleepPlace == 2u.toUByte()
            })
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "feed.solids" && it.solidsFoods == listOf("Apple", "Rice") &&
                    it.solidsAmount == "half bowl"
            })
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "feed.breast" && it.breastSegments?.map { segment -> segment.side } ==
                    listOf(2u.toUByte(), 1u.toUByte(), 2u.toUByte()) &&
                    it.breastSegments?.first()?.endUtcMs == it.startUtcMs + 6 * 60_000L
            })
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "pump" && it.pumpTotalMl == 40L &&
                    it.pumpLeftMl == null && it.pumpRightMl == null
            })
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "medication" && it.medicationName == "Corrected medicine" &&
                    it.medicationDoseAmount == "3" && it.medicationDoseUnit == "mL"
            })
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "growth" && it.growthWeightG == 4_300L && it.growthLengthMm == 540L &&
                    it.growthHeadMm == 355L
            })
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "temperature" && it.temperatureC == "37.22" &&
                    it.temperatureEntered == "99" && it.temperatureUnit == 31u.toUByte()
            })
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "diaper" && it.diaperKind == 2u.toUByte()
            })
            assertTrue(sharing.snapshot(family).activities.count { it.kind == "sleep" && it.endUtcMs != null } == 2)
            assertTrue(sharing.snapshot(family).activities.any {
                it.kind == "sleep" && it.endUtcMs != null &&
                    it.endUtcMs!! - it.startUtcMs == 20 * 60_000L
            })
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
