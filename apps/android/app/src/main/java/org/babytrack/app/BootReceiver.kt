package org.babytrack.app

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import uniffi.babytrack_core_ffi.NativeLocalStore

/** Restore local timer visibility after reboot, independently of network sync. */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED) return
        val pending = goAsync()
        Thread({
            try {
                SharedSyncJobService.schedule(context)
                refreshSleepTimers(context, context.filesDir.resolve("families.db").absolutePath)
            } catch (failure: Exception) {
                Log.w("BabytrackBoot", "Could not restore timer notification", failure)
            } finally {
                pending.finish()
            }
        }, "babytrack-boot-refresh").start()
    }
}

internal fun refreshSleepTimers(context: Context, databasePath: String) {
    NativeLocalStore.open(databasePath).use { local ->
        ShareCoordinator(context, databasePath).use { sharing ->
            SleepTimerNotifications.update(
                context,
                runningSleepCount(local, sharing, local.families(), sharing.recipientFamilies()),
            )
            refreshLiveTimerNotifications(context, local, sharing)
        }
    }
}
