package org.babytrack.app

import android.os.Build
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.babytrack_core_ffi.UniffiCleaner

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [26], manifest = Config.NONE)
class BindingCleanerCompatibilityTest {
    @Test
    fun api26UsesCompatibleCleanupAndRunsItOnlyOnce() {
        assertEquals(26, Build.VERSION.SDK_INT)
        // Exercise the pinned generated factory without opening a Rust store.
        val factory =
            Class.forName("uniffi.babytrack_core_ffi.Babytrack_core_ffiKt")
                .getDeclaredMethod("create", UniffiCleaner.Companion::class.java)
                .apply { isAccessible = true }
        val cleaner = factory.invoke(null, UniffiCleaner.Companion) as UniffiCleaner
        assertEquals("UniffiJnaCleaner", cleaner.javaClass.simpleName)
        val retainedObject = Any()
        var cleanupCount = 0
        val handle = cleaner.register(retainedObject, Runnable { cleanupCount++ })
        handle.clean()
        handle.clean()
        assertEquals(1, cleanupCount)
    }
}
