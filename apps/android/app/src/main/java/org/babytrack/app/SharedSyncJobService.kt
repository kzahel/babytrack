package org.babytrack.app

import android.app.job.JobInfo
import android.app.job.JobParameters
import android.app.job.JobScheduler
import android.app.job.JobService
import android.content.ComponentName
import android.content.Context
import android.util.Log
import uniffi.babytrack_core_ffi.NativeLocalStore
import java.util.concurrent.Executors
import java.util.concurrent.Future

/** A deferrable network wake. The signed relay log remains the sync source. */
class SharedSyncJobService : JobService() {
    private val worker = Executors.newSingleThreadExecutor()
    private var running: Future<*>? = null
    @Volatile private var stopped = false

    override fun onStartJob(params: JobParameters): Boolean {
        stopped = false
        running = worker.submit {
            var failed = false
            try {
                val database = filesDir.resolve("families.db")
                val origins = getSharedPreferences("shared_relay_origins", MODE_PRIVATE)
                NativeLocalStore.open(database.absolutePath).use { local ->
                    ShareCoordinator(this, database.absolutePath).use { sharing ->
                        for (family in local.families()) {
                            if (Thread.currentThread().isInterrupted) return@use
                            val origin = origins.getString(family.familyId.keyForJob(), null)
                            if (origin != null && sharing.isShared(family) && !sharing.isRemoved(family)) {
                                runCatching { sharing.advanceManager(family, origin) }
                                    // The foreground rereads the durable removal-copy mapping.
                                    .onFailure { if (it !is VerifiedManagerRemoval) failed = true }
                            }
                        }
                        for (family in sharing.recipientFamilies()) {
                            if (Thread.currentThread().isInterrupted) return@use
                            runCatching { sharing.advanceRecipient(family) }
                                .onFailure { if (it !is InvitationTerminal) failed = true }
                        }
                        runCatching {
                            SleepTimerNotifications.update(
                                this,
                                runningSleeps(local, sharing, local.families(), sharing.recipientFamilies()),
                            )
                        }.onFailure { Log.w("BabytrackTimer", "Could not refresh sleep notification", it) }
                        runCatching { refreshLiveTimerNotifications(this, local, sharing) }
                            .onFailure { Log.w("BabytrackTimer", "Could not refresh feeding notifications", it) }
                    }
                }
            } catch (_: Exception) {
                failed = true
            } finally {
                getSharedPreferences("shared_background_sync", MODE_PRIVATE).edit()
                    .putLong("last_attempt_ms", System.currentTimeMillis())
                    .putBoolean("last_failed", failed)
                    .apply()
                if (!stopped) jobFinished(params, failed)
            }
        }
        return true
    }

    override fun onStopJob(params: JobParameters): Boolean {
        stopped = true
        running?.cancel(true)
        return true
    }

    override fun onDestroy() {
        stopped = true
        worker.shutdownNow()
        super.onDestroy()
    }

    companion object {
        private const val JOB_ID = 3400
        private const val INTERVAL_MS = 15 * 60 * 1000L

        fun schedule(context: Context) {
            val scheduler = context.getSystemService(JobScheduler::class.java)
            if (scheduler.getPendingJob(JOB_ID) != null) return
            val job = JobInfo.Builder(JOB_ID, ComponentName(context, SharedSyncJobService::class.java))
                .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
                .setPeriodic(INTERVAL_MS)
                .setPersisted(true)
                .build()
            check(scheduler.schedule(job) == JobScheduler.RESULT_SUCCESS)
        }
    }
}

private fun ByteArray.keyForJob(): String = joinToString("") { "%02x".format(it) }
