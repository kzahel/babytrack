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
    private const val notificationId = 4101

    fun update(context: Context, activeCount: Int) {
        SleepTimerWidget.update(context, activeCount)
        val manager = context.getSystemService(NotificationManager::class.java)
        if (activeCount == 0) {
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
        val openTracker = PendingIntent.getActivity(
            context,
            0,
            Intent(context, MainActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
            },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val message = if (activeCount == 1) context.getString(R.string.sleep_notification_one)
            else context.getString(R.string.sleep_notification_many, activeCount)
        val notification = Notification.Builder(context, channelId)
            .setSmallIcon(android.R.drawable.ic_lock_idle_alarm)
            .setContentTitle(context.getString(R.string.sleep_notification_title))
            .setContentText(message)
            .setContentIntent(openTracker)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setVisibility(Notification.VISIBILITY_PRIVATE)
            .build()
        manager.notify(notificationId, notification)
    }
}
