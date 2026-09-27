package org.babytrack.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.babytrack_core_ffi.NativeLocalStore
import uniffi.babytrack_core_ffi.NativeSharedStore
import uniffi.babytrack_core_ffi.ActivityWhen

@RunWith(AndroidJUnit4::class)
class SharingRelayTest {
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
            sharing.addChild(first.family, "Offline shared child", System.currentTimeMillis())
            val now = System.currentTimeMillis()
            assertTrue(runCatching {
                sharing.logDiaper(first.family, ByteArray(16), 1u.toUByte(), ActivityWhen(now, 0, now))
            }.isFailure)
            sharing.logDiaper(first.family, existing.id, 1u.toUByte(), ActivityWhen(now, 0, now))
            assertEquals(2, sharing.snapshot(first.family).children.size)
            assertEquals(1, sharing.snapshot(first.family).activities.size)
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
            assertTrue(sharing.syncAndUpload(first.family, origin).ready)
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
