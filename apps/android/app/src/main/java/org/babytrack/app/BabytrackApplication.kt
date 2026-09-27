package org.babytrack.app

import android.app.Application
import uniffi.babytrack_core_ffi.NativeLocalStore

/** One native store per app process; Activity recreation never closes in-flight sync. */
class BabytrackApplication : Application() {
    private val databasePath by lazy { filesDir.resolve("families.db").absolutePath }
    val localStore: NativeLocalStore by lazy { NativeLocalStore.open(databasePath) }
    internal val sharing: ShareCoordinator by lazy { ShareCoordinator(this, databasePath) }
}
