package org.babytrack.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.babytrack_core_ffi.FamilyRef
import uniffi.babytrack_core_ffi.NativeLocalStore
import uniffi.babytrack_core_ffi.NativeSharedStore

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
                local.addChild(it, "Shared child", System.currentTimeMillis())
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
            assertTrue(sharing.snapshot(family).children.any { it.name == "Shared child" })
            sharing.addChild(family, "Recipient child", System.currentTimeMillis())
            assertTrue(sharing.syncRecipientAndUpload(family).ready)
            assertEquals(0uL, sharing.snapshot(family).unsentCount)
        }
    }

    @Test fun managerVerifyAndUpload() {
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            val family = family()
            assertTrue(sharing.syncAndUpload(family, origin).ready)
            assertTrue(sharing.snapshot(family).children.any { it.name == "Recipient child" })
            sharing.addChild(family, "Manager child", System.currentTimeMillis())
            assertTrue(sharing.syncAndUpload(family, origin).ready)
        }
    }

    @Test fun recipientVerifyAndSaveOffline() {
        val family = ShareCoordinator(context, db.absolutePath).use { sharing ->
            val family = sharing.recipientFamilies().single()
            assertTrue(sharing.syncRecipientAndUpload(family).ready)
            assertTrue(sharing.snapshot(family).children.any { it.name == "Manager child" })
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
