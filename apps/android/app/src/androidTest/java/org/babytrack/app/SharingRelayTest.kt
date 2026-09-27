package org.babytrack.app

import android.view.accessibility.AccessibilityNodeInfo
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import android.app.job.JobScheduler
import android.os.ParcelFileDescriptor
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.babytrack_core_ffi.NativeLocalStore
import uniffi.babytrack_core_ffi.NativeSharedStore
import uniffi.babytrack_core_ffi.ActivityWhen
import uniffi.babytrack_core_ffi.previewInvitation

@RunWith(AndroidJUnit4::class)
class SharingRelayTest {
    @Test
    fun activityRecreationResumesSavedRecipientClaim() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("activity-manager-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(managerDb.absolutePath).use { it.createFamily(System.currentTimeMillis()) }
        val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
        }
        val preview = previewInvitation(fragment)
        val wrapping = DeviceWrappingKey(context).loadOrCreate()
        val recipient = try {
            NativeSharedStore.open(context.filesDir.resolve("families.db").absolutePath).use { core ->
                core.prepareJoin(fragment, RelayTransport(origin).get(preview.controlPath, preview.readAuth), wrapping).family
            }
        } finally {
            wrapping.fill(0)
        }
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            scenario.recreate()
            val deadline = System.currentTimeMillis() + 12_000
            var resumed = false
            var visible = false
            while (System.currentTimeMillis() < deadline) {
                val root = instrumentation.uiAutomation.rootInActiveWindow
                visible = root?.containsText("Today") == true
                val wrappingAfter = DeviceWrappingKey(context).loadOrCreate()
                resumed = try {
                    NativeSharedStore.open(context.filesDir.resolve("families.db").absolutePath).use { core ->
                        core.recipientFirstJoinAction(recipient, wrappingAfter) == 0u.toUByte()
                    }
                } finally {
                    wrappingAfter.fill(0)
                }
                if (visible && resumed) break
                Thread.sleep(100)
            }
            assertTrue("Recreated UI should resume and confirm the saved claim (visible=$visible, resumed=$resumed)", visible && resumed)
        }
        ShareCoordinator(context, context.filesDir.resolve("families.db").absolutePath).use { sharing ->
            assertTrue(sharing.recipientFamilies().any { it.familyId.contentEquals(recipient.familyId) })
        }
    }

    private fun AccessibilityNodeInfo.containsText(text: String): Boolean {
        if (this.text?.toString()?.contains(text) == true) return true
        return (0 until childCount).any { index -> getChild(index)?.containsText(text) == true }
    }

    @Test
    fun savedClaimRetriesByFamilyAfterPrecommitFailureOrLostResponse() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        for (committedBeforeRestart in listOf(false, true)) {
            val managerDb = context.filesDir.resolve("claim-manager-${System.nanoTime()}.db")
            val recipientDb = context.filesDir.resolve("claim-recipient-${System.nanoTime()}.db")
            val family = NativeLocalStore.open(managerDb.absolutePath).use { local ->
                local.createFamily(System.currentTimeMillis())
            }
            val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
                sharing.promote(family, origin, publicKey)
                sharing.invite(family, origin, 1u.toUByte())
            }
            val preview = previewInvitation(fragment)
            val relay = RelayTransport(origin)
            val wrapping = DeviceWrappingKey(context).loadOrCreate()
            val prepared = try {
                NativeSharedStore.open(recipientDb.absolutePath).use { core ->
                    val page = relay.get(preview.controlPath, preview.readAuth)
                    core.prepareJoin(fragment, page, wrapping)
                }
            } finally {
                wrapping.fill(0)
            }
            if (committedBeforeRestart) {
                relay.post("/v1/families/${family.familyId.joinToString("") { "%02x".format(it) }}/control", prepared.candidateBytes)
                // Drop the signed response before the local claim is confirmed.
            }
            ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
                assertArrayEquals(prepared.family.familyId, sharing.recipientFamilies().single().familyId)
                assertEquals(origin, sharing.recipientOrigin(prepared.family))
                assertTrue(sharing.advanceRecipient(prepared.family).awaitingGrant)
                assertArrayEquals(prepared.candidateBytes, sharing.retryClaim(prepared.family).candidateBytes)
            }
        }
    }

    @Test
    fun scheduledJobUploadsSavedManagerEdit() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val database = context.filesDir.resolve("families.db")
        val family = NativeLocalStore.open(database.absolutePath).use { local ->
            local.createFamily(System.currentTimeMillis())
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.addChild(family, "Background child", System.currentTimeMillis())
            assertEquals(1uL, sharing.snapshot(family).unsentCount)
        }
        context.getSharedPreferences("shared_relay_origins", android.content.Context.MODE_PRIVATE)
            .edit().putString(family.familyId.joinToString("") { "%02x".format(it) }, origin).commit()
        SharedSyncJobService.schedule(context)
        assertTrue(context.getSystemService(JobScheduler::class.java).getPendingJob(3400) != null)
        val before = context.getSharedPreferences("shared_background_sync", android.content.Context.MODE_PRIVATE)
            .getLong("last_attempt_ms", 0)
        val command = instrumentation.uiAutomation.executeShellCommand("cmd jobscheduler run -f org.babytrack.app 3400")
        ParcelFileDescriptor.AutoCloseInputStream(command).bufferedReader().use { it.readText() }
        val deadline = System.currentTimeMillis() + 15_000
        while (System.currentTimeMillis() < deadline) {
            val done = context.getSharedPreferences("shared_background_sync", android.content.Context.MODE_PRIVATE)
                .getLong("last_attempt_ms", 0) > before
            if (done) break
            Thread.sleep(100)
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            assertEquals(0uL, sharing.snapshot(family).unsentCount)
        }
    }

    @Test
    fun asynchronousJoinAdvancesWithoutManualHolderApproval() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("auto-manager-${System.nanoTime()}.db")
        val recipientDb = context.filesDir.resolve("auto-recipient-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(managerDb.absolutePath).use { local ->
            val created = local.createFamily(System.currentTimeMillis())
            local.addChild(created, "Async child", System.currentTimeMillis())
            created
        }
        val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
        }
        val recipient = ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            sharing.claim(fragment).family
        }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceManager(family, origin).ready)
        }
        ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceRecipient(recipient).awaitingGrant)
        }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceManager(family, origin).ready)
        }
        ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceRecipient(recipient).ready)
            assertEquals("Async child", sharing.snapshot(recipient).children.single().name)
        }
    }

    @Test
    fun keystoreWrappedPromotionAndInvitationSurviveRestart() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val firstKey = DeviceWrappingKey(context).loadOrCreate()
        val reopenedKey = DeviceWrappingKey(context).loadOrCreate()
        assertArrayEquals(firstKey, reopenedKey)
        firstKey.fill(0)
        reopenedKey.fill(0)

        val database = context.filesDir.resolve("sharing-test-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(database.absolutePath).use { local ->
            val family = local.createFamily(System.currentTimeMillis())
            local.addChild(family, "Relay test child", System.currentTimeMillis())
            family
        }
        val fragment = ShareCoordinator(context, database.absolutePath).use { sharing ->
            assertEquals(1uL, sharing.promote(family, origin, publicKey))
            assertEquals("Relay test child", sharing.snapshot(family).children.single().name)
            assertEquals(1, sharing.snapshot(family).devices.size)
            NativeLocalStore.open(database.absolutePath).use { local ->
                assertTrue(runCatching { local.addChild(family, "Wrong surface", System.currentTimeMillis()) }.isFailure)
            }
            val fragment = sharing.invite(family, origin, 1u.toUByte())
            assertTrue(fragment.startsWith("#bt-invite=v1."))
            fragment
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            assertEquals(2uL, sharing.promote(family, origin, publicKey))
            assertTrue(sharing.invite(family, origin, 1u.toUByte()).startsWith("#bt-invite=v1."))
        }
        val recipient = context.filesDir.resolve("recipient-test-${System.nanoTime()}.db")
        val first = ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            sharing.claim(fragment)
        }
        assertArrayEquals(family.familyId, first.family.familyId)
        val retried = ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            sharing.claim(fragment)
        }
        assertEquals(first.family.deviceId.toList(), retried.family.deviceId.toList())
        assertArrayEquals(first.candidateBytes, retried.candidateBytes)
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            assertArrayEquals(first.family.familyId, sharing.recipientFamilies().single().familyId)
            assertEquals(origin, sharing.recipientOrigin(first.family))
        }
        NativeLocalStore.open(recipient.absolutePath).use { local ->
            assertTrue(local.families().none { it.familyId.contentEquals(family.familyId) })
            assertTrue(
                runCatching {
                    local.addChild(first.family, "Too early", System.currentTimeMillis())
                }.isFailure,
            )
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            sharing.respondToClaim(family, origin)
        }
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            sharing.proveChallenge(first.family)
        }
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            val pending = sharing.syncRecipient(first.family)
            assertTrue(pending.awaitingGrant)
            assertTrue(!pending.ready)
            assertTrue(runCatching { sharing.snapshot(first.family) }.isFailure)
            assertTrue(runCatching { sharing.addChild(first.family, "Too early", System.currentTimeMillis()) }.isFailure)
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            sharing.admitProvedDevice(family, origin)
        }
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            val synced = sharing.syncRecipient(first.family)
            assertTrue(synced.ready)
            assertEquals(1uL, synced.childCount)
            val existing = sharing.snapshot(first.family).children.single()
            assertEquals("Relay test child", existing.name)
            assertEquals(2, sharing.snapshot(first.family).devices.size)
            assertTrue(sharing.snapshot(first.family).devices.any {
                it.deviceId.contentEquals(first.family.deviceId) && it.role == 1u.toUByte()
            })
            sharing.addChild(first.family, "Offline shared child", System.currentTimeMillis())
            val now = System.currentTimeMillis()
            assertTrue(runCatching {
                sharing.logDiaper(first.family, ByteArray(16), 1u.toUByte(), ActivityWhen(now, 0, now))
            }.isFailure)
            sharing.logDiaper(first.family, existing.id, 1u.toUByte(), ActivityWhen(now, 0, now))
            assertEquals(2, sharing.snapshot(first.family).children.size)
            assertEquals(1, sharing.snapshot(first.family).activities.size)
            assertEquals(2uL, sharing.snapshot(first.family).unsentCount)
            assertEquals(0uL, sharing.snapshot(first.family).inertCount)
            val file = sharing.backupFile(first.family, now, null, 512_000_000uL)
            assertArrayEquals(first.family.familyId, file.info.sourceFamilyId)
            assertEquals(4uL, file.info.recordCount)
            val protected = sharing.backupFile(first.family, now, "backup secret", 512_000_000uL)
            NativeLocalStore.open(context.filesDir.resolve("restored-${System.nanoTime()}.db").absolutePath).use { local ->
                assertEquals(4uL, local.inspectProtected(protected.bytes, "backup secret", 512_000_000uL).recordCount)
                assertTrue(runCatching {
                    local.inspectProtected(protected.bytes, "wrong secret", 512_000_000uL)
                }.isFailure)
                val restored = local.restore(file.bytes, now)
                assertEquals(2, local.children(restored).size)
            }
            val copy = sharing.privateCopy(first.family, now)
            assertArrayEquals(copy.familyId, sharing.privateCopy(first.family, now + 1).familyId)
            assertTrue(!sharing.isShared(copy))
            NativeLocalStore.open(recipient.absolutePath).use { local ->
                assertEquals(2, local.children(copy).size)
            }
        }
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            assertTrue(sharing.syncRecipient(first.family).ready)
            assertEquals(2, sharing.snapshot(first.family).children.size)
            assertEquals(1, sharing.snapshot(first.family).activities.size)
            val wrapping = DeviceWrappingKey(context).loadOrCreate()
            try {
                NativeSharedStore.open(recipient.absolutePath).use { core ->
                    val candidate = core.prepareSharedUpload(first.family, wrapping)
                        ?: error("Expected an offline batch")
                    val familyHex = first.family.familyId.joinToString("") { "%02x".format(it.toInt() and 255) }
                    RelayTransport(origin).post("/v1/families/$familyHex/batches", candidate)
                    // Drop the response: the next sync must discover its signed acceptance.
                }
            } finally {
                wrapping.fill(0)
            }
            val uploaded = sharing.syncAndUpload(first.family, origin)
            assertTrue(uploaded.ready)
            assertEquals(0u.toUByte(), uploaded.outboxState)
            assertEquals(0uL, sharing.snapshot(first.family).unsentCount)
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            assertTrue(sharing.syncAndUpload(family, origin).ready)
            assertEquals(2, sharing.snapshot(family).children.size)
            assertEquals(1, sharing.snapshot(family).activities.size)
            sharing.addChild(family, "Manager later", System.currentTimeMillis())
            assertTrue(sharing.syncAndUpload(family, origin).ready)
        }
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            assertTrue(sharing.syncAndUpload(first.family, origin).ready)
            assertEquals(3, sharing.snapshot(first.family).children.size)
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            sharing.admitProvedDevice(family, origin)
        }
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            sharing.proveChallenge(fragment)
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            sharing.respondToClaim(family, origin)
        }
    }
}
