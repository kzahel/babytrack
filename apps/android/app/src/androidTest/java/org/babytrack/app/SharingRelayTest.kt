package org.babytrack.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.babytrack_core_ffi.NativeLocalStore

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
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            assertEquals(1uL, sharing.promote(family, origin, publicKey))
            val fragment = sharing.invite(family, origin, 1u.toUByte())
            assertTrue(fragment.startsWith("#bt-invite=v1."))
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            assertEquals(2uL, sharing.promote(family, origin, publicKey))
            assertTrue(sharing.invite(family, origin, 1u.toUByte()).startsWith("#bt-invite=v1."))
        }
    }
}
