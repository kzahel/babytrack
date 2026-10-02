package org.babytrack.app

import android.Manifest
import android.app.Notification
import android.app.NotificationManager
import android.content.Context
import android.os.Build
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.isDisplayed
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.performClick
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class TimerNotificationsTest {
    @get:Rule val composeRule = createEmptyComposeRule()
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private val context get() = instrumentation.targetContext
    private val app get() = context.applicationContext as BabytrackApplication
    private val manager get() = context.getSystemService(NotificationManager::class.java)
    private val timers get() = liveTimerStore(context)

    @Before
    fun prepare() {
        context.getSharedPreferences("live_timers", Context.MODE_PRIVATE).edit().clear().commit()
        manager.cancelAll()
        if (Build.VERSION.SDK_INT >= 33)
            instrumentation.uiAutomation.grantRuntimePermission(context.packageName, Manifest.permission.POST_NOTIFICATIONS)
    }

    private fun posted(session: TimerNotificationTarget): Notification? =
        manager.activeNotifications.find { it.tag == LiveTimerNotifications.tag(session) }?.notification

    private fun refresh() = refreshLiveTimerNotifications(context, app.localStore, app.sharing)

    private fun waitForCapture(name: String) {
        composeRule.waitUntil(10_000) {
            runCatching {
                composeRule.onNodeWithText(context.getString(R.string.capture_for_child, name)).isDisplayed()
            }.getOrDefault(false)
        }
    }

    @Test
    fun coldAndWarmNotificationTapsOpenTheExactTimerAndChild() {
        val now = System.currentTimeMillis()
        val first = app.localStore.createFamily(now)
        val firstChild = app.localStore.addChild(first, "Nursing notification child", now)
        val second = app.localStore.createFamily(now)
        val secondChild = app.localStore.addChild(second, "Pumping notification child", now)
        val nursingTarget = timerTarget(first.familyId.key(), firstChild.key())
        val pumpTarget = timerTarget(second.familyId.key(), secondChild.key())
        timers.saveNursing(nursingTarget, listOf(TimedSegment(1u, now - 120_000L, null)))
        timers.savePumpStart(pumpTarget, now - 60_000L)
        val nursing = timers.session(nursingTarget, LiveTimerKind.NURSING)!!
        val pump = timers.session(pumpTarget, LiveTimerKind.PUMP)!!
        context.getSharedPreferences("tracker_selection", Context.MODE_PRIVATE).edit()
            .putString("family", second.familyId.key()).putString("child", secondChild.key()).commit()
        refresh()
        composeRule.waitUntil(10_000) { posted(nursing) != null && posted(pump) != null }
        assertTrue(posted(nursing)!!.extras.getBoolean(Notification.EXTRA_SHOW_CHRONOMETER))
        ActivityScenario.launch<MainActivity>(LiveTimerNotifications.openIntent(context, nursing)).use { scenario ->
            waitForCapture("Nursing notification child")
            composeRule.onNodeWithText(context.getString(R.string.timer_running)).assertIsDisplayed()
            scenario.recreate()
            waitForCapture("Nursing notification child")
            composeRule.onNodeWithText(context.getString(R.string.capture_for_child, "Nursing notification child"))
                .assertIsDisplayed()
            posted(pump)!!.contentIntent.send()
            waitForCapture("Pumping notification child")
            composeRule.onNodeWithText(context.getString(R.string.pump_timer_stop)).assertIsDisplayed().performClick()
            composeRule.waitUntil(10_000) { posted(pump) == null }
            assertNotNull(posted(nursing))
            posted(nursing)!!.contentIntent.send()
            waitForCapture("Nursing notification child")
            composeRule.onNodeWithText(context.getString(R.string.save_breast)).performClick()
            composeRule.waitUntil(10_000) { posted(nursing) == null }
            assertTrue(timers.nursing(nursingTarget).isEmpty())
            assertEquals(1, app.localStore.timeline(first, firstChild).count { it.kind == "feed.breast" })
        }
    }

    @Test
    fun bootRefreshRestoresDraftsAndDismissalDoesNotDiscardThem() {
        val now = System.currentTimeMillis()
        val family = app.localStore.createFamily(now)
        val child = app.localStore.addChild(family, "Restored timer child", now)
        val target = timerTarget(family.familyId.key(), child.key())
        val segments = listOf(TimedSegment(1u, now - 120_000L, now - 60_000L))
        timers.saveNursing(target, segments)
        val session = timers.session(target, LiveTimerKind.NURSING)!!
        // The boot receiver uses this same offline refresh path.
        refreshSleepTimers(context, context.filesDir.resolve("families.db").absolutePath)
        composeRule.waitUntil(10_000) { posted(session) != null }
        assertFalse(posted(session)!!.extras.getBoolean(Notification.EXTRA_SHOW_CHRONOMETER))
        posted(session)!!.deleteIntent.send()
        composeRule.waitUntil(10_000) { posted(session) == null }
        refreshSleepTimers(context, context.filesDir.resolve("families.db").absolutePath)
        assertNull(posted(session))
        assertEquals(segments, timers.nursing(target))
    }

    @Test
    fun rejectedNursingSaveKeepsItsDraftAndNotification() {
        val now = System.currentTimeMillis()
        val family = app.localStore.createFamily(now)
        val child = app.localStore.addChild(family, "Long nursing child", now)
        val target = timerTarget(family.familyId.key(), child.key())
        val segments = listOf(TimedSegment(1u, now - 241 * 60_000L, null))
        timers.saveNursing(target, segments)
        val session = timers.session(target, LiveTimerKind.NURSING)!!
        refresh()
        ActivityScenario.launch<MainActivity>(LiveTimerNotifications.openIntent(context, session)).use {
            waitForCapture("Long nursing child")
            composeRule.onNodeWithText(context.getString(R.string.save_breast)).performClick()
            composeRule.waitUntil(10_000) {
                composeRule.onAllNodesWithText(context.getString(R.string.breast_timer_invalid))
                    .fetchSemanticsNodes().isNotEmpty()
            }
            assertEquals(segments, timers.nursing(target))
            assertNotNull(posted(session))
            assertTrue(app.localStore.timeline(family, child).isEmpty())
        }
    }

    @Test
    fun staleNotificationCannotReopenAReplacementDraft() {
        val now = System.currentTimeMillis()
        val family = app.localStore.createFamily(now)
        val child = app.localStore.addChild(family, "Replacement timer child", now)
        val target = timerTarget(family.familyId.key(), child.key())
        timers.savePumpStart(target, now - 60_000L)
        val old = timers.session(target, LiveTimerKind.PUMP)!!
        timers.savePumpStart(target, now)
        ActivityScenario.launch<MainActivity>(LiveTimerNotifications.openIntent(context, old)).use {
            composeRule.waitUntil(10_000) {
                composeRule.onAllNodesWithText(context.getString(R.string.timer_notification_expired))
                    .fetchSemanticsNodes().isNotEmpty()
            }
            assertEquals(now, timers.pumpStart(target))
            composeRule.onNodeWithText(context.getString(R.string.nav_today)).assertIsDisplayed()
        }
    }
}
