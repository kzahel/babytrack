package org.babytrack.app

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build

/** A tap returns to the tracker; timer state stays in the Rust journal. */
internal object SleepTimerNotifications {
    private const val channelId = "sleep_timers"
    internal const val notificationId = 4101

    internal fun notification(context: Context, sleeps: RunningSleeps): Notification {
        val openTracker = PendingIntent.getActivity(
            context,
            0,
            Intent(context, MainActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
            },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val single = sleeps.count == 1
        val title = when {
            !single -> context.getString(R.string.sleep_widget_many, sleeps.count)
            sleeps.childName != null -> context.getString(R.string.sleep_notification_child, sleeps.childName)
            else -> context.getString(R.string.sleep_notification_title)
        }
        val message = if (single && sleeps.earliestStartMs != null) {
            context.getString(R.string.sleep_notification_since, clockTime(context, sleeps.earliestStartMs))
        } else context.getString(R.string.sleep_notification_one)
        // The lock screen shows neither the child's name nor the start time.
        val public = Notification.Builder(context, channelId)
            .setSmallIcon(R.drawable.ic_sleep_notification)
            .setContentTitle(context.getString(R.string.timer_notification_private))
            .setVisibility(Notification.VISIBILITY_PUBLIC)
            .build()
        val clock = single && sleeps.earliestStartMs != null
        return Notification.Builder(context, channelId)
            .setSmallIcon(R.drawable.ic_sleep_notification)
            .setContentTitle(title)
            .setContentText(message)
            .setContentIntent(openTracker)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setCategory(if (Build.VERSION.SDK_INT >= 31) Notification.CATEGORY_STOPWATCH else Notification.CATEGORY_STATUS)
            .setVisibility(Notification.VISIBILITY_PRIVATE)
            .setPublicVersion(public)
            .setWhen(sleeps.earliestStartMs ?: System.currentTimeMillis())
            .setShowWhen(clock)
            .setUsesChronometer(clock)
            .build()
    }

    fun update(context: Context, sleeps: RunningSleeps) {
        SleepTimerWidget.update(context, sleeps.count)
        val manager = context.getSystemService(NotificationManager::class.java)
        if (sleeps.count == 0) {
            manager.cancel(notificationId)
            return
        }
        if (Build.VERSION.SDK_INT >= 33 &&
            context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) return

        manager.createNotificationChannel(NotificationChannel(
            channelId,
            context.getString(R.string.sleep_notification_channel),
            NotificationManager.IMPORTANCE_LOW,
        ))
        manager.notify(notificationId, notification(context, sleeps))
    }
}
