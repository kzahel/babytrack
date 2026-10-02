package org.babytrack.app

import android.Manifest
import android.app.Application
import android.app.Notification
import android.app.NotificationManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], application = Application::class)
class LiveTimerNotificationsTest {
    private val context = RuntimeEnvironment.getApplication()
    private val store = liveTimerStore(context)
    private val family = "a".repeat(32)
    private val child = "b".repeat(32)
    private val target = timerTarget(family, child)
    private val minute = 60_000L
    private val start = 1_000_000L
    private val manager = context.getSystemService(NotificationManager::class.java)

    private fun grant() = shadowOf(context).grantPermissions(Manifest.permission.POST_NOTIFICATIONS)

    @Test
    fun nursingChronometerUsesActiveTimeAndFreezesWhilePaused() {
        grant()
        store.saveNursing(target, listOf(
            TimedSegment(1u, start, start + 5 * minute),
            TimedSegment(2u, start + 8 * minute, null),
        ))
        val session = store.session(target, LiveTimerKind.NURSING)!!
        val now = start + 10 * minute
        LiveTimerNotifications.update(context, store, mapOf(target to "Child"), now)
        val running = manager.activeNotifications.single().notification
        assertTrue(running.extras.getBoolean(Notification.EXTRA_SHOW_CHRONOMETER))
        assertEquals(now - 7 * minute, running.`when`)
        assertEquals(context.getString(R.string.nursing_notification_running,
            context.getString(R.string.breast_right)), running.extras.getString(Notification.EXTRA_TEXT))
        assertEquals(Notification.VISIBILITY_PRIVATE, running.visibility)
        assertEquals(context.getString(R.string.timer_notification_private),
            running.publicVersion.extras.getString(Notification.EXTRA_TITLE))
        assertFalse(running.publicVersion.extras.toString().contains("Child"))

        store.saveNursing(target, tapNursingSide(store.nursing(target), 2u, now))
        LiveTimerNotifications.update(context, store, mapOf(target to "Child"), now + 20 * minute)
        val paused = manager.activeNotifications.single().notification
        assertFalse(paused.extras.getBoolean(Notification.EXTRA_SHOW_CHRONOMETER))
        assertEquals(context.getString(R.string.nursing_notification_paused, elapsedClock(7 * minute)),
            paused.extras.getString(Notification.EXTRA_TEXT))
        assertEquals(0, paused.flags and Notification.FLAG_ONGOING_EVENT)
        assertEquals(session, timerNotificationTarget(shadowOf(paused.contentIntent).savedIntent))

        store.saveNursing(target, tapNursingSide(store.nursing(target), 1u, now + 20 * minute))
        LiveTimerNotifications.update(context, store, mapOf(target to "Child"), now + 22 * minute)
        assertEquals(now + 22 * minute - 9 * minute, manager.activeNotifications.single().notification.`when`)
    }

    @Test
    fun simultaneousTargetsAndTypesKeepDistinctIntentsAndClearIndependently() {
        grant()
        val other = timerTarget("c".repeat(32), "d".repeat(32))
        store.saveNursing(target, listOf(TimedSegment(1u, start, null)))
        store.savePumpStart(target, start)
        store.savePumpStart(other, start)
        val names = mapOf(target to "First", other to "Second")
        LiveTimerNotifications.update(context, store, names, start + minute)
        val posted = manager.activeNotifications
        assertEquals(3, posted.size)
        assertEquals(3, posted.map { it.notification.contentIntent }.distinct().size)
        assertEquals(3, posted.map { timerNotificationTarget(shadowOf(it.notification.contentIntent).savedIntent) }.distinct().size)
        store.savePumpStart(target, null)
        LiveTimerNotifications.update(context, store, names, start + minute)
        assertEquals(2, manager.activeNotifications.size)
        store.saveNursing(target, emptyList())
        LiveTimerNotifications.update(context, store, names, start + minute)
        assertEquals(LiveTimerKind.PUMP,
            timerNotificationTarget(shadowOf(manager.activeNotifications.single().notification.contentIntent).savedIntent)!!.kind)
    }

    @Test
    fun dismissalSurvivesRefreshAndCannotDismissReplacementSession() {
        grant()
        val segments = listOf(TimedSegment(1u, start, null))
        store.saveNursing(target, segments)
        LiveTimerNotifications.update(context, store, mapOf(target to "Child"), start + minute)
        val posted = manager.activeNotifications.single().notification
        TimerNotificationDismissReceiver().onReceive(context, shadowOf(posted.deleteIntent).savedIntent)
        assertEquals(segments, store.nursing(target))
        LiveTimerNotifications.update(context, liveTimerStore(context), mapOf(target to "Child"), start + minute)
        assertTrue(manager.activeNotifications.isEmpty())
        val old = store.session(target, LiveTimerKind.NURSING)!!
        store.saveNursing(target, emptyList())
        store.saveNursing(target, listOf(TimedSegment(1u, start + minute, null)))
        LiveTimerNotifications.update(context, store, mapOf(target to "Child"), start + minute)
        LiveTimerNotifications.dismiss(context, old)
        assertEquals(1, manager.activeNotifications.size)
        assertFalse(store.matches(old))
    }

    @Test
    fun unavailableTargetsHideNotificationsAndRetainDrafts() {
        grant()
        store.savePumpStart(target, start)
        LiveTimerNotifications.update(context, store, mapOf(target to "Child"), start + minute)
        LiveTimerNotifications.update(context, store, emptyMap(), start + minute)
        assertTrue(manager.activeNotifications.isEmpty())
        assertEquals(start, store.pumpStart(target))
    }

    @Test
    fun deniedPermissionLeavesDraftsAndGrantRebuildsNotifications() {
        shadowOf(context).denyPermissions(Manifest.permission.POST_NOTIFICATIONS)
        store.savePumpStart(target, start)
        assertTrue(shouldRequestTimerNotifications(context))
        assertFalse(shouldRequestTimerNotifications(context))
        LiveTimerNotifications.update(context, store, mapOf(target to "Child"), start + minute)
        assertTrue(manager.activeNotifications.isEmpty())
        assertEquals(start, store.pumpStart(target))
        grant()
        assertFalse(shouldRequestTimerNotifications(context))
        LiveTimerNotifications.update(context, store, mapOf(target to "Child"), start + minute)
        assertEquals(1, manager.activeNotifications.size)
    }

    @Test
    @Config(sdk = [26])
    fun olderAndroidPostsWithoutRuntimePermission() {
        store.savePumpStart(target, start)
        assertFalse(shouldRequestTimerNotifications(context))
        LiveTimerNotifications.update(context, store, mapOf(target to "Child"), start + minute)
        assertEquals(1, manager.activeNotifications.size)
    }

    @Test
    fun malformedTargetsAndStaleIntentsAreRejected() {
        val valid = TimerNotificationTarget(family, child, LiveTimerKind.PUMP, start)
        assertEquals(valid, timerNotificationTarget(Intent().setData(valid.uri)))
        for (uri in listOf("babytrack-timer://PUMP/invalid/$child/$start",
            "babytrack-timer://PUMP/$family/$child/-1", "babytrack-timer://OTHER/$family/$child/$start",
            "babytrack-timer://PUMP/$family/$child/$start?extra=1")) {
            assertNull(timerNotificationTarget(Intent().setData(Uri.parse(uri))))
        }
        store.savePumpStart(target, start)
        assertTrue(store.matches(valid))
        store.savePumpStart(target, null)
        assertFalse(store.matches(valid))
    }
}
