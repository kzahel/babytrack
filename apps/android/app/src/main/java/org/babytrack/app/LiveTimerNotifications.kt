package org.babytrack.app

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import uniffi.babytrack_core_ffi.NativeLocalStore

internal enum class LiveTimerKind { NURSING, PUMP }

/** A notification can reopen only the exact local draft it was posted for. */
internal data class TimerNotificationTarget(
    val familyKey: String,
    val childKey: String,
    val kind: LiveTimerKind,
    val startMs: Long,
) {
    val target: String get() = timerTarget(familyKey, childKey)
    val uri: Uri get() = Uri.Builder().scheme("babytrack-timer").authority(kind.name)
        .appendPath(familyKey).appendPath(childKey).appendPath(startMs.toString()).build()
}

internal fun timerNotificationTarget(intent: Intent?): TimerNotificationTarget? {
    val uri = intent?.data ?: return null
    if (uri.scheme != "babytrack-timer" || uri.query != null || uri.fragment != null ||
        uri.port != -1 || uri.userInfo != null) return null
    val kind = LiveTimerKind.entries.find { it.name == uri.host } ?: return null
    val parts = uri.pathSegments
    if (parts.size != 3 || parts.take(2).any { !it.matches(Regex("[0-9a-f]{32}")) }) return null
    val start = parts[2].toLongOrNull()?.takeIf { it > 0L } ?: return null
    return TimerNotificationTarget(parts[0], parts[1], kind, start)
}

internal fun liveTimerStore(context: Context): LiveTimerStore =
    LiveTimerStore(context.getSharedPreferences("live_timers", Context.MODE_PRIVATE))

/** Ask at the first useful moment, without prompting again on every side switch. */
internal fun shouldRequestTimerNotifications(context: Context): Boolean {
    if (Build.VERSION.SDK_INT < 33 || context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) ==
        PackageManager.PERMISSION_GRANTED) return false
    val prefs = context.getSharedPreferences("timer_notification_permission", Context.MODE_PRIVATE)
    if (prefs.getBoolean("asked", false)) return false
    prefs.edit().putBoolean("asked", true).apply()
    return true
}

/** Rebuild from durable drafts and current core-owned Family/child state, offline. */
internal fun refreshLiveTimerNotifications(
    context: Context,
    local: NativeLocalStore,
    sharing: ShareCoordinator,
) {
    val names = mutableMapOf<String, String>()
    val store = liveTimerStore(context)
    val targetFamilies = store.targets().mapTo(mutableSetOf()) { it.substringBefore(':') }
    val recipients = sharing.recipientFamilies()
    val recipientKeys = recipients.mapTo(mutableSetOf()) { it.familyId.key() }
    val families = (local.families() + recipients).distinctBy { it.familyId.key() }
    for (family in families) {
        if (family.familyId.key() !in targetFamilies) continue
        val children = if (family.familyId.key() in recipientKeys || sharing.isShared(family)) {
            // Unavailable drafts stay on disk; notifications never migrate their target.
            if (sharing.isRemoved(family)) continue
            runCatching { sharing.snapshot(family).children }.getOrNull() ?: continue
        } else local.children(family)
        for (child in children) names[timerTarget(family.familyId.key(), child.id.key())] = child.name
    }
    LiveTimerNotifications.update(context, store, names)
}

internal object LiveTimerNotifications {
    private const val channelId = "feeding_timers"
    private const val notificationId = 4102
    private const val tagPrefix = "live-timer:"

    fun tag(session: TimerNotificationTarget): String = "$tagPrefix${session.kind.name}:${session.target}"

    fun openIntent(context: Context, session: TimerNotificationTarget): Intent =
        Intent(context, MainActivity::class.java).apply {
            data = session.uri
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }

    internal fun notification(
        context: Context,
        store: LiveTimerStore,
        session: TimerNotificationTarget,
        childName: String,
        nowMs: Long,
    ): Notification {
        val segments = if (session.kind == LiveTimerKind.NURSING) store.nursing(session.target) else emptyList()
        val running = session.kind == LiveTimerKind.PUMP || segments.running() != null
        val elapsed = if (session.kind == LiveTimerKind.NURSING) totalElapsedMs(segments, nowMs)
            else (nowMs - session.startMs).coerceAtLeast(0L)
        val title = context.getString(
            if (session.kind == LiveTimerKind.NURSING) R.string.nursing_notification_title
            else R.string.pump_notification_title,
            childName,
        )
        val message = when {
            !running -> context.getString(R.string.nursing_notification_paused, elapsedClock(elapsed))
            session.kind == LiveTimerKind.PUMP -> context.getString(R.string.pump_notification_running)
            else -> context.getString(R.string.nursing_notification_running, context.getString(
                if (segments.running()?.side == 1u.toUByte()) R.string.breast_left else R.string.breast_right,
            ))
        }
        val open = PendingIntent.getActivity(context, 0, openIntent(context, session),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val dismiss = PendingIntent.getBroadcast(context, 0,
            Intent(context, TimerNotificationDismissReceiver::class.java).setData(session.uri),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val public = Notification.Builder(context, channelId)
            .setSmallIcon(R.drawable.ic_timer_notification)
            .setContentTitle(context.getString(R.string.timer_notification_private))
            .setVisibility(Notification.VISIBILITY_PUBLIC)
            .build()
        return Notification.Builder(context, channelId)
            .setSmallIcon(R.drawable.ic_timer_notification)
            .setContentTitle(title)
            .setContentText(message)
            .setContentIntent(open)
            .setDeleteIntent(dismiss)
            .setOngoing(running)
            .setOnlyAlertOnce(true)
            .setCategory(if (Build.VERSION.SDK_INT >= 31) Notification.CATEGORY_STOPWATCH else Notification.CATEGORY_STATUS)
            .setVisibility(Notification.VISIBILITY_PRIVATE)
            .setPublicVersion(public)
            .setWhen(nowMs - elapsed)
            .setShowWhen(running)
            .setUsesChronometer(running)
            .build()
    }

    @Synchronized
    fun update(context: Context, store: LiveTimerStore, childNames: Map<String, String>, nowMs: Long = System.currentTimeMillis()) {
        val manager = context.getSystemService(NotificationManager::class.java)
        val permitted = Build.VERSION.SDK_INT < 33 ||
            context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
        val sessions = if (permitted) store.targets().flatMap { target ->
            LiveTimerKind.entries.mapNotNull { kind -> store.session(target, kind) }
        }.filter { it.target in childNames && !store.isDismissed(it) } else emptyList()
        val tags = sessions.mapTo(mutableSetOf()) { tag(it) }
        for (posted in manager.activeNotifications) {
            if (posted.tag?.startsWith(tagPrefix) == true && posted.tag !in tags)
                manager.cancel(posted.tag, notificationId)
        }
        if (sessions.isEmpty()) return
        manager.createNotificationChannel(NotificationChannel(channelId,
            context.getString(R.string.feeding_notification_channel), NotificationManager.IMPORTANCE_LOW).apply {
                setSound(null, null)
                enableVibration(false)
                setShowBadge(false)
            })
        for (session in sessions) manager.notify(tag(session), notificationId,
            notification(context, store, session, childNames.getValue(session.target), nowMs))
    }

    @Synchronized
    fun dismiss(context: Context, session: TimerNotificationTarget) {
        val store = liveTimerStore(context)
        if (!store.matches(session)) return
        store.dismiss(session)
        context.getSystemService(NotificationManager::class.java).cancel(tag(session), notificationId)
    }
}

/** Dismissal hides a notification; it never stops or discards the draft. */
class TimerNotificationDismissReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        timerNotificationTarget(intent)?.let { LiveTimerNotifications.dismiss(context, it) }
    }
}
