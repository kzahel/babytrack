package org.babytrack.app

import android.view.accessibility.AccessibilityNodeInfo
import android.content.Intent
import android.Manifest
import android.app.NotificationManager
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import android.app.job.JobScheduler
import android.os.ParcelFileDescriptor
import android.widget.FrameLayout
import android.widget.TextView
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.printToString
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.babytrack_core_ffi.NativeLocalStore
import uniffi.babytrack_core_ffi.NativeSharedStore
import uniffi.babytrack_core_ffi.ActivityWhen
import uniffi.babytrack_core_ffi.previewInvitation
import java.time.LocalDate

@RunWith(AndroidJUnit4::class)
class SharingRelayTest {
    @get:Rule val composeRule = createEmptyComposeRule()

    private fun ByteArray.hex(): String = joinToString("") { "%02x".format(it.toInt() and 255) }

    @Before
    fun acknowledgeEarlierRemovalCopies() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val database = context.filesDir.resolve("families.db")
        val acknowledgments = context.getSharedPreferences(
            "acknowledged_removal_copies", android.content.Context.MODE_PRIVATE,
        )
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            NativeLocalStore.open(database.absolutePath).use { local ->
                val editor = acknowledgments.edit()
                for (source in local.families() + sharing.recipientFamilies()) {
                    val copy = sharing.savedRemovalCopy(source) ?: continue
                    editor.putString(source.familyId.hex(), copy.familyId.hex())
                }
                assertTrue(editor.commit())
            }
        }
    }

    private fun openTab(label: Int) {
        val text = InstrumentationRegistry.getInstrumentation().targetContext.getString(label)
        val tab = hasText(text) and SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Tab)
        composeRule.waitUntil(25_000) {
            composeRule.onAllNodes(tab).fetchSemanticsNodes().isNotEmpty()
        }
        composeRule.onNode(tab).performSemanticsAction(SemanticsActions.OnClick)
        composeRule.waitUntil(25_000) {
            composeRule.onAllNodes(tab).fetchSemanticsNodes().singleOrNull()
                ?.let { runCatching { it.config[SemanticsProperties.Selected] }.getOrDefault(false) } == true
        }
    }

    private fun openFirstEntryActions() {
        composeRule.waitUntil(25_000) {
            composeRule.onAllNodesWithTag("entry-row").fetchSemanticsNodes().isNotEmpty()
        }
        composeRule.onAllNodesWithTag("entry-row").onFirst().performScrollTo().performClick()
    }

    @Test
    fun childCreationOpensProfileAndSavesName() {
        wakeEmulatorScreen()
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val database = context.filesDir.resolve("families.db")
        val family = NativeLocalStore.open(database.absolutePath).use {
            it.createFamily(System.currentTimeMillis())
        }
        context.getSharedPreferences("tracker_selection", android.content.Context.MODE_PRIVATE)
            .edit().putString("family", family.familyId.hex()).remove("child").commit()
        ActivityScenario.launch(MainActivity::class.java).use {
            composeRule.onNodeWithText(context.getString(R.string.add_child))
                .performScrollTo().performClick()
            composeRule.onNodeWithText(context.getString(R.string.create_child_profile))
                .assertIsDisplayed()
            composeRule.onNodeWithText(context.getString(R.string.child_age_unknown))
                .assertIsDisplayed()
            composeRule.onNodeWithText(context.getString(R.string.child_name))
                .performTextInput("Profile child")
            composeRule.onAllNodesWithText(context.getString(R.string.add_child)).onLast().performClick()
            composeRule.waitUntil(25_000) {
                NativeLocalStore.open(database.absolutePath).use { local ->
                    local.children(family).any { child -> child.name == "Profile child" }
                }
            }
        }
    }

    @Test
    fun childEditShowsAgeAndSavesNameAndSex() {
        wakeEmulatorScreen()
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val database = context.filesDir.resolve("families.db")
        val birthDay = LocalDate.now().minusDays(10).toEpochDay()
        val (family, childId) = NativeLocalStore.open(database.absolutePath).use { local ->
            val family = local.createFamily(System.currentTimeMillis())
            family to local.addChildWithMetadata(family, "Before edit", birthDay, 2u.toUByte(),
                System.currentTimeMillis())
        }
        context.getSharedPreferences("tracker_selection", android.content.Context.MODE_PRIVATE)
            .edit().putString("family", family.familyId.hex()).putString("child", childId.hex()).commit()
        ActivityScenario.launch(MainActivity::class.java).use {
            openTab(R.string.nav_family)
            composeRule.onNodeWithText(context.getString(R.string.child_options))
                .performScrollTo().performClick()
            composeRule.onNodeWithText(context.getString(R.string.edit_child_profile))
                .performScrollTo().performClick()
            composeRule.onAllNodesWithText(
                context.resources.getQuantityString(R.plurals.child_age_days, 10, 10),
            ).onLast().assertIsDisplayed()
            composeRule.onNodeWithText(context.getString(R.string.child_name))
                .performTextReplacement("After edit")
            composeRule.onNodeWithText(context.getString(R.string.sex_female))
                .performScrollTo().performClick()
            composeRule.onNodeWithText(context.getString(R.string.save_changes)).performClick()
            composeRule.waitUntil(25_000) {
                NativeLocalStore.open(database.absolutePath).use { local ->
                    local.children(family).any { child -> child.id.contentEquals(childId) &&
                        child.name == "After edit" && child.birthDay == birthDay &&
                        child.sex == 1u.toUByte() }
                }
            }
        }
    }

    @Test
    fun commaDecimalBottleSavesThroughCanonicalCoreInput() {
        wakeEmulatorScreen()
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val db = context.filesDir.resolve("families.db")
        val now = System.currentTimeMillis()
        val (family, child) = NativeLocalStore.open(db.absolutePath).use { local ->
            val family = local.createFamily(now)
            family to local.addChild(family, "Comma child", now)
        }
        context.getSharedPreferences("tracker_selection", android.content.Context.MODE_PRIVATE)
            .edit().putString("family", family.familyId.hex())
            .putString("child", child.hex()).commit()
        ActivityScenario.launch(MainActivity::class.java).use {
            val addActivity = context.getString(R.string.add_activity)
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText(addActivity)
                    .fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(addActivity)
                .performScrollTo().performClick()
            composeRule.onNodeWithText(context.getString(R.string.event_bottle)).performClick()
            composeRule.onNodeWithText(context.getString(R.string.unit_us_fl_oz))
                .performScrollTo().performClick()
            composeRule.onNodeWithText(context.getString(R.string.bottle_amount))
                .performScrollTo().performTextInput("4,5")
            composeRule.onNodeWithText(context.getString(R.string.save_bottle)).performClick()
            openTab(R.string.nav_history)
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText("Bottle · 4.5 US fl oz · Formula")
                    .fetchSemanticsNodes().isNotEmpty()
            }
        }
        NativeLocalStore.open(db.absolutePath).use { local ->
            assertTrue(local.timeline(family, child).any {
                it.bottleEntered == "4.5" && it.bottleUnit == 2u.toUByte()
            })
        }
    }

    @Test
    fun headerTargetPickerChangesTheHistoryChild() {
        wakeEmulatorScreen()
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val db = context.filesDir.resolve("families.db")
        val now = System.currentTimeMillis()
        val (family, first, _) = NativeLocalStore.open(db.absolutePath).use { local ->
            val family = local.createFamily(now)
            val first = local.addChild(family, "First child", now)
            val second = local.addChild(family, "Second child", now)
            local.logNote(family, first, "First marker", ActivityWhen(now, 0, now))
            local.logNote(family, second, "Second marker", ActivityWhen(now, 0, now))
            Triple(family, first, second)
        }
        context.getSharedPreferences("tracker_selection", android.content.Context.MODE_PRIVATE)
            .edit().putString("family", family.familyId.hex())
            .putString("child", first.hex()).commit()
        ActivityScenario.launch(MainActivity::class.java).use {
            val switch = context.getString(R.string.switch_target)
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText("First child")
                    .fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithContentDescription(switch).performClick()
            composeRule.onNodeWithText("Second child").performClick()
            openTab(R.string.nav_history)
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText("Note · Second marker")
                    .fetchSemanticsNodes().isNotEmpty()
            }
            assertTrue(composeRule.onAllNodesWithText("Note · First marker")
                .fetchSemanticsNodes().isEmpty())
        }
    }

    @Test
    fun changingChildDiscardsAnUnsentCaptureDraft() {
        wakeEmulatorScreen()
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val db = context.filesDir.resolve("families.db")
        val now = System.currentTimeMillis()
        val (family, first, second) = NativeLocalStore.open(db.absolutePath).use { local ->
            val family = local.createFamily(now)
            val first = local.addChild(family, "Draft first", now)
            val second = local.addChild(family, "Draft second", now)
            Triple(family, first, second)
        }
        context.getSharedPreferences("tracker_selection", android.content.Context.MODE_PRIVATE)
            .edit().putString("family", family.familyId.hex())
            .putString("child", first.hex()).commit()
        ActivityScenario.launch(MainActivity::class.java).use {
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText("Draft first", substring = true)
                    .fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(context.getString(R.string.add_activity))
                .performScrollTo().performClick()
            composeRule.onNodeWithText(context.getString(R.string.event_bottle))
                .performScrollTo().performClick()
            composeRule.onNode(hasSetTextAction()).performTextInput("47")
            composeRule.onNodeWithContentDescription(context.getString(R.string.back)).performClick()
            composeRule.onNodeWithContentDescription(context.getString(R.string.switch_target))
                .performClick()
            composeRule.onNodeWithText("Draft second")
                .performSemanticsAction(SemanticsActions.OnClick)
            composeRule.waitUntil(25_000) {
                context.getSharedPreferences("tracker_selection", android.content.Context.MODE_PRIVATE)
                    .getString("child", null) == second.hex()
            }
            composeRule.waitForIdle()
            composeRule.onNodeWithText(context.getString(R.string.add_activity))
                .performScrollTo().performClick()
            composeRule.onNodeWithText(context.getString(R.string.event_bottle))
                .performScrollTo().performClick()
            val draft = composeRule.onNode(hasSetTextAction()).fetchSemanticsNode()
                .config[SemanticsProperties.EditableText].text
            assertEquals("", draft)
        }
    }

    @Test
    fun deletedEntryCanBeUndoneFromTheTracker() {
        wakeEmulatorScreen()
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val db = context.filesDir.resolve("families.db")
        val now = System.currentTimeMillis()
        val (family, child, note) = NativeLocalStore.open(db.absolutePath).use { local ->
            val family = local.createFamily(now)
            val child = local.addChild(family, "Undo child", now)
            val note = local.logNote(family, child, "Undo me", ActivityWhen(now, 0, now))
            Triple(family, child, note)
        }
        context.getSharedPreferences("tracker_selection", android.content.Context.MODE_PRIVATE)
            .edit().putString("family", family.familyId.joinToString("") { "%02x".format(it) })
            .putString("child", child.joinToString("") { "%02x".format(it) }).commit()
        ActivityScenario.launch(MainActivity::class.java).use {
            openTab(R.string.nav_history)
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText("Note · Undo me").fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText("Note · Undo me").performScrollTo().assertIsDisplayed()
            openFirstEntryActions()
            composeRule.onNodeWithText(context.getString(R.string.delete_entry))
                .performScrollTo().performClick()
            composeRule.onNodeWithText(context.getString(R.string.confirm_delete_entry)).performClick()
            composeRule.waitUntil(15_000) {
                composeRule.onAllNodesWithText(context.getString(R.string.undo))
                    .fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(context.getString(R.string.undo)).performClick()
            composeRule.waitUntil(15_000) {
                composeRule.onAllNodesWithText("Note · Undo me").fetchSemanticsNodes().isNotEmpty()
            }
        }
        NativeLocalStore.open(db.absolutePath).use { local ->
            assertTrue(local.timeline(family, child).any { it.id.contentEquals(note) })
        }
    }

    @Test
    fun selectedChildTodaySummaryUsesSavedLocalEntries() {
        wakeEmulatorScreen()
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        InstrumentationRegistry.getInstrumentation().uiAutomation.grantRuntimePermission(
            context.packageName, Manifest.permission.POST_NOTIFICATIONS)
        val db = context.filesDir.resolve("families.db")
        val now = System.currentTimeMillis()
        val (family, child, timer) = NativeLocalStore.open(db.absolutePath).use { local ->
            val family = local.createFamily(now)
            val child = local.addChild(family, "Summary child", now)
            local.logBottleMl(family, child, 90, 2u.toUByte(), ActivityWhen(now, 0, now))
            local.logDiaper(family, child, 3u.toUByte(), ActivityWhen(now, 0, now))
            val timer = local.startSleep(family, child, ActivityWhen(now - 600_000L, 0, now))
            local.addChild(family, "Other summary child", now)
            Triple(family, child, timer)
        }
        context.getSharedPreferences("tracker_selection", android.content.Context.MODE_PRIVATE)
            .edit().putString("family", family.familyId.joinToString("") { "%02x".format(it) })
            .putString("child", child.joinToString("") { "%02x".format(it) }).commit()
        ActivityScenario.launch(MainActivity::class.java).use {
            val feeds = context.resources.getQuantityString(R.plurals.today_feeds, 1, 1L, 90L)
            val diapers = context.resources.getQuantityString(R.plurals.today_diapers, 1, 1L, 1L, 1L)
            val lastTime = compactDateTime(context, now, now)
            val sleepTime = compactDateTime(context, now - 600_000L, now)
            val bottle = context.getString(R.string.bottle_with_entered, "90",
                context.getString(R.string.unit_ml), context.getString(R.string.bottle_formula))
            val diaper = context.getString(R.string.diaper, context.getString(R.string.both))
            val lastFeed = context.getString(R.string.tile_detail_at, bottle, lastTime)
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText(feeds).fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(feeds).performScrollTo().assertIsDisplayed()
            composeRule.onNodeWithText(diapers).performScrollTo().assertIsDisplayed()
            composeRule.onNodeWithText(lastFeed).performScrollTo().assertIsDisplayed()
            composeRule.onNodeWithText(context.getString(R.string.tile_detail_at, diaper, lastTime))
                .performScrollTo().assertIsDisplayed()
            composeRule.onNodeWithText(context.getString(R.string.running_sleep_since,
                sleepTime)).performScrollTo().assertIsDisplayed()
            composeRule.onAllNodesWithText(context.getString(R.string.stop_sleep)).onFirst()
                .performScrollTo().performClick()
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText(context.getString(R.string.running_sleep_since,
                    sleepTime)).fetchSemanticsNodes().isEmpty()
            }
            composeRule.onAllNodesWithText(context.getString(R.string.start_sleep)).onFirst()
                .performScrollTo().performClick()
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText(context.getString(R.string.stop_sleep))
                    .fetchSemanticsNodes().isNotEmpty()
            }
            openTab(R.string.nav_family)
            composeRule.onNodeWithText("Other summary child").performScrollTo().performClick()
            openTab(R.string.nav_today)
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText(context.getString(R.string.today_empty))
                    .fetchSemanticsNodes().isNotEmpty()
            }
            assertTrue(composeRule.onAllNodesWithText(lastFeed).fetchSemanticsNodes().isEmpty())
        }
        NativeLocalStore.open(db.absolutePath).use { local ->
            assertTrue(local.timeline(family, child).any {
                it.id.contentEquals(timer) && it.endUtcMs != null
            })
            assertTrue(local.timeline(family, child).any {
                it.id.contentEquals(timer).not() && it.kind == "sleep" && it.endUtcMs == null
            })
        }
    }

    @Test
    fun savedDiaperTimeEditOpensAndSavesThroughTracker() {
        wakeEmulatorScreen()
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val db = context.filesDir.resolve("families.db")
        val now = System.currentTimeMillis()
        val (family, child, activity) = NativeLocalStore.open(db.absolutePath).use { local ->
            val family = local.createFamily(now)
            val child = local.addChild(family, "Time edit child", now)
            val activity = local.logDiaper(family, child, 1u.toUByte(),
                ActivityWhen(now - 3_600_000L, 180, now))
            Triple(family, child, activity)
        }
        context.getSharedPreferences("tracker_selection", android.content.Context.MODE_PRIVATE)
            .edit().putString("family", family.familyId.joinToString("") { "%02x".format(it) })
            .putString("child", child.joinToString("") { "%02x".format(it) }).commit()
        ActivityScenario.launch(MainActivity::class.java).use {
            openTab(R.string.nav_history)
            val edit = context.getString(R.string.edit_entry_time)
            openFirstEntryActions()
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText(edit).fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(edit).performScrollTo().performClick()
            composeRule.onNodeWithText(context.getString(R.string.choose_entry_time)).assertIsDisplayed()
            composeRule.onNodeWithText(context.getString(R.string.save_changes)).performClick()
        }
        NativeLocalStore.open(db.absolutePath).use { local ->
            assertTrue(local.timeline(family, child).any {
                it.id.contentEquals(activity) && it.startUtcMs == now - 3_600_000L &&
                    it.offsetMinutes == 180.toShort()
            })
        }
    }

    @Test
    fun completedSleepMoveRequiresChoosingAStartTime() {
        wakeEmulatorScreen()
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val db = context.filesDir.resolve("families.db")
        val now = System.currentTimeMillis()
        val originalStart = now - 60 * 60_000L - now % 60_000L + 45_000L
        val (family, child, activity) = NativeLocalStore.open(db.absolutePath).use { local ->
            val family = local.createFamily(now)
            val child = local.addChild(family, "Move sleep child", now)
            val activity = local.logSleep(family, child,
                ActivityWhen(originalStart, 0, now), originalStart + 20 * 60_000L, 0)
            Triple(family, child, activity)
        }
        context.getSharedPreferences("tracker_selection", android.content.Context.MODE_PRIVATE)
            .edit().putString("family", family.familyId.joinToString("") { "%02x".format(it) })
            .putString("child", child.joinToString("") { "%02x".format(it) }).commit()
        ActivityScenario.launch(MainActivity::class.java).use {
            openTab(R.string.nav_history)
            val move = context.getString(R.string.move_completed_session)
            openFirstEntryActions()
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText(move).fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(move).performScrollTo().performClick()
            composeRule.onNodeWithText(context.getString(R.string.choose_entry_time)).assertIsDisplayed()
            composeRule.onNodeWithText(context.getString(R.string.save_changes)).assertIsNotEnabled()
            composeRule.onNodeWithText(context.getString(R.string.choose_entry_time)).performClick()
            clickNativePositiveDialogButton()
            clickNativePositiveDialogButton()
            composeRule.onNodeWithText(context.getString(R.string.save_changes)).performClick()
        }
        NativeLocalStore.open(db.absolutePath).use { local ->
            assertTrue(local.timeline(family, child).any {
                it.id.contentEquals(activity) && it.startUtcMs != originalStart &&
                    it.startUtcMs in (originalStart - 60_000L)..originalStart &&
                    it.endUtcMs == it.startUtcMs + 20 * 60_000L
            })
        }
    }

    private fun clickNativePositiveDialogButton() {
        val automation = InstrumentationRegistry.getInstrumentation().uiAutomation
        val deadline = System.currentTimeMillis() + 10_000
        while (System.currentTimeMillis() < deadline) {
            val button = automation.rootInActiveWindow
                ?.findAccessibilityNodeInfosByViewId("android:id/button1")?.firstOrNull()
            if (button != null && button.performAction(AccessibilityNodeInfo.ACTION_CLICK)) {
                Thread.sleep(250)
                return
            }
            Thread.sleep(50)
        }
        error("Native date or time picker confirmation did not appear")
    }

    @Test
    fun localTimerNotificationTracksSavedStartAndStop() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        instrumentation.uiAutomation.grantRuntimePermission(
            context.packageName, Manifest.permission.POST_NOTIFICATIONS,
        )
        val path = context.filesDir.resolve("timer-notification-${System.nanoTime()}.db")
        val manager = context.getSystemService(NotificationManager::class.java)
        NativeLocalStore.open(path.absolutePath).use { local ->
            ShareCoordinator(context, path.absolutePath).use { sharing ->
                val now = System.currentTimeMillis()
                val family = local.createFamily(now)
                val child = local.addChild(family, "Timer child", now)
                val activity = local.startSleep(family, child, ActivityWhen(now, 0, now))
                val running = runningSleepCount(local, sharing, listOf(family), emptyList())
                assertEquals(1, running)
                assertEquals(
                    context.getString(R.string.sleep_widget_one),
                    SleepTimerWidget.views(context, running).apply(context, FrameLayout(context))
                        .findViewById<TextView>(R.id.widget_status).text.toString(),
                )
                SleepTimerNotifications.update(context, running)
                assertTrue("Running timer notification should appear", waitForSleepNotification(manager, context, true))
                SleepTimerNotifications.update(context, 0)
                assertTrue("Cleared notification should disappear", waitForSleepNotification(manager, context, false))
                refreshSleepTimers(context, path.absolutePath)
                assertTrue("Saved timer should restore its notification", waitForSleepNotification(manager, context, true))
                local.stopSleep(family, child, activity, now + 60_000, 0, now + 60_000)
                val stopped = runningSleepCount(local, sharing, listOf(family), emptyList())
                assertEquals(0, stopped)
                assertEquals(
                    context.getString(R.string.sleep_widget_none),
                    SleepTimerWidget.views(context, stopped).apply(context, FrameLayout(context))
                        .findViewById<TextView>(R.id.widget_status).text.toString(),
                )
                SleepTimerNotifications.update(context, stopped)
                assertTrue("Stopped timer notification should clear", waitForSleepNotification(manager, context, false))
            }
        }
    }

    private fun waitForSleepNotification(
        manager: NotificationManager,
        context: android.content.Context,
        expected: Boolean,
    ): Boolean {
        val deadline = System.currentTimeMillis() + 10_000
        do {
            val present = manager.activeNotifications.any {
                it.notification.extras.getString("android.title") == context.getString(R.string.sleep_notification_title)
            }
            if (present == expected) return true
            Thread.sleep(50)
        } while (System.currentTimeMillis() < deadline)
        return false
    }

    @Test
    fun claimedLinkIsTerminalButSavedCompetingClaimStaysUnknown() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("claimed-manager-${System.nanoTime()}.db")
        val firstDb = context.filesDir.resolve("claimed-first-${System.nanoTime()}.db")
        val secondDb = context.filesDir.resolve("claimed-second-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(managerDb.absolutePath).use {
            it.createFamily(System.currentTimeMillis())
        }
        val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
        }
        val preview = previewInvitation(fragment)
        val wrapping = DeviceWrappingKey(context).loadOrCreate()
        val waiting = try {
            NativeSharedStore.open(secondDb.absolutePath).use { core ->
                core.prepareJoin(fragment, RelayTransport(origin).get(preview.controlPath, preview.readAuth), wrapping).family
            }
        } finally {
            wrapping.fill(0)
        }
        ShareCoordinator(context, firstDb.absolutePath).use { sharing ->
            sharing.claim(fragment)
        }
        ShareCoordinator(context, context.filesDir.resolve("claimed-link-${System.nanoTime()}.db").absolutePath).use { sharing ->
            val result = runCatching { sharing.claim(fragment) }
            assertEquals(InvitationTerminalReason.CLAIMED, (result.exceptionOrNull() as? InvitationTerminal)?.reason)
        }
        ShareCoordinator(context, secondDb.absolutePath).use { sharing ->
            val result = runCatching { sharing.advanceRecipient(waiting) }
            assertTrue(result.isFailure)
            assertTrue(result.exceptionOrNull() !is InvitationTerminal)
            assertTrue(sharing.recipientFamilies().any { it.familyId.contentEquals(waiting.familyId) })
        }
        ShareCoordinator(context, secondDb.absolutePath).use { sharing ->
            val result = runCatching { sharing.advanceRecipient(waiting) }
            assertTrue(result.isFailure)
            assertTrue(result.exceptionOrNull() !is InvitationTerminal)
        }
    }

    @Test
    fun committedRetryWithLostResponseRemainsRetryableAfterRestart() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("lost-retry-manager-${System.nanoTime()}.db")
        val recipientDb = context.filesDir.resolve("lost-retry-recipient-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(managerDb.absolutePath).use { local ->
            val created = local.createFamily(System.currentTimeMillis())
            local.addChild(created, "Recovered join child", System.currentTimeMillis())
            created
        }
        val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
        }
        val preview = previewInvitation(fragment)
        val wrapping = DeviceWrappingKey(context).loadOrCreate()
        val recipient = try {
            NativeSharedStore.open(recipientDb.absolutePath).use { core ->
                core.prepareJoin(fragment, RelayTransport(origin).get(preview.controlPath, preview.readAuth), wrapping).family
            }
        } finally {
            wrapping.fill(0)
        }
        var dropOnce = true
        val relayProvider: (String) -> RelayTransport = { relayOrigin ->
            object : RelayTransport(relayOrigin) {
                override fun post(path: String, bytes: ByteArray, allowBatchConflict: Boolean): ByteArray {
                    val accepted = super.post(path, bytes, allowBatchConflict)
                    if (dropOnce && path.endsWith("/control")) {
                        dropOnce = false
                        throw IllegalStateException("simulated lost accepted claim response")
                    }
                    return accepted
                }
            }
        }
        ShareCoordinator(context, recipientDb.absolutePath, relayProvider).use { sharing ->
            val failure = runCatching { sharing.advanceRecipient(recipient) }.exceptionOrNull()
            assertEquals("simulated lost accepted claim response", failure?.message)
        }
        val afterLostResponse = DeviceWrappingKey(context).loadOrCreate()
        try {
            NativeSharedStore.open(recipientDb.absolutePath).use { core ->
                assertEquals(null, core.savedJoinTerminalStatus(recipient, afterLostResponse))
                assertEquals(2u.toUByte(), core.recipientFirstJoinAction(recipient, afterLostResponse))
            }
        } finally {
            afterLostResponse.fill(0)
        }
        ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceRecipient(recipient).awaitingGrant)
        }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceManager(family, origin).ready)
        }
        ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceRecipient(recipient).awaitingGrant)
        }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceManager(family, origin).ready)
        }
        ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceRecipient(recipient).ready)
            assertEquals("Recovered join child", sharing.snapshot(recipient).children.single().name)
        }
    }

    @Test
    fun managerStopsPendingJoinAfterLostRemovalResponse() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("pending-remove-manager-${System.nanoTime()}.db")
        val recipientDb = context.filesDir.resolve("pending-remove-recipient-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(managerDb.absolutePath).use { local ->
            val created = local.createFamily(System.currentTimeMillis())
            local.addChild(created, "Still with manager", System.currentTimeMillis())
            created
        }
        val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
        }
        val recipient = ShareCoordinator(context, recipientDb.absolutePath).use { it.claim(fragment).family }
        val pending = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            assertTrue(sharing.syncAndUpload(family, origin).ready)
            sharing.snapshot(family).pendingDevices.single()
        }
        assertArrayEquals(recipient.deviceId, pending.deviceId)
        val wrapping = DeviceWrappingKey(context).loadOrCreate()
        try {
            val prepared = NativeSharedStore.open(managerDb.absolutePath).use { core ->
                core.preparePendingRemoval(family, wrapping, pending.invitationId, pending.deviceId)
            }
            RelayTransport(origin).post(
                "/v1/families/${family.familyId.joinToString("") { "%02x".format(it.toInt() and 255) }}/control",
                prepared.candidateBytes,
            ) // The committed response is lost before local confirmation.
        } finally {
            wrapping.fill(0)
        }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            val after = sharing.removePendingDevice(family, origin, pending.invitationId, pending.deviceId)
            assertTrue(after.pendingDevices.isEmpty())
            assertEquals(1, after.devices.size)
            assertEquals("Still with manager", after.children.single().name)
            assertTrue(sharing.syncAndUpload(family, origin).ready)
        }
        ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            val stopped = sharing.advanceRecipient(recipient)
            assertEquals(8u.toUByte(), stopped.joinPhase)
            assertTrue(!stopped.awaitingGrant && !stopped.ready)
            assertTrue(runCatching { sharing.snapshot(recipient) }.isFailure)
        }
        ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            assertEquals(8u.toUByte(), sharing.advanceRecipient(recipient).joinPhase)
        }
    }

    @Test
    fun managerStopsPendingDeviceFromAccessUi() {
        wakeEmulatorScreen()
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("families.db")
        val recipientDb = context.filesDir.resolve("pending-ui-recipient-${System.nanoTime()}.db")
        val (family, child) = NativeLocalStore.open(managerDb.absolutePath).use { local ->
            val created = local.createFamily(System.currentTimeMillis())
            created to local.addChild(created, "Everyday child", System.currentTimeMillis())
        }
        val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
        }
        val recipient = ShareCoordinator(context, recipientDb.absolutePath).use { it.claim(fragment).family }
        val pending = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            assertTrue(sharing.syncAndUpload(family, origin).ready)
            sharing.snapshot(family).pendingDevices.single()
        }
        val familyHex = family.familyId.joinToString("") { "%02x".format(it.toInt() and 255) }
        context.getSharedPreferences("shared_relay_origins", android.content.Context.MODE_PRIVATE)
            .edit().putString(familyHex, origin).commit()
        context.getSharedPreferences("tracker_selection", android.content.Context.MODE_PRIVATE)
            .edit().putString("family", familyHex).putString("child", child.hex()).commit()
        ActivityScenario.launch(MainActivity::class.java).use {
            openTab(R.string.nav_family)
            val shortId = pending.deviceId.joinToString("") { "%02x".format(it.toInt() and 255) }.take(8)
            val label = context.getString(R.string.device_short_id, shortId)
            val button = context.getString(R.string.remove_pending_device, label)
            val familyAccess = context.getString(R.string.show_family_access)
            try {
                composeRule.waitUntil(25_000) {
                    composeRule.onAllNodesWithText(familyAccess).fetchSemanticsNodes().isNotEmpty()
                }
            } catch (failure: Throwable) {
                throw AssertionError(
                    "Family access missing for ${familyHex.take(8)}; " +
                        composeRule.onRoot().printToString(), failure,
                )
            }
            composeRule.onNodeWithText(context.resources.getQuantityString(
                R.plurals.shared_pending_device_count, 1, 1)).performScrollTo().assertIsDisplayed()
            assertTrue(composeRule.onAllNodesWithText(button).fetchSemanticsNodes().isEmpty())
            assertTrue(composeRule.onAllNodesWithText("Everyday child", substring = true)
                .fetchSemanticsNodes().isNotEmpty())
            composeRule.onNodeWithText(context.getString(R.string.show_family_access))
                .performScrollTo().performClick()
            composeRule.onNodeWithText(button).performScrollTo().performClick()
            val confirm = context.getString(R.string.confirm_remove_pending_device)
            composeRule.waitUntil(15_000) {
                composeRule.onAllNodesWithText(confirm).fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(confirm).performClick()
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText(context.getString(R.string.pending_device_removed))
                    .fetchSemanticsNodes().isNotEmpty()
            }
        }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            assertTrue(sharing.snapshot(family).pendingDevices.isEmpty())
        }
        ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            assertEquals(8u.toUByte(), sharing.advanceRecipient(recipient).joinPhase)
        }
    }

    @Test
    fun removedRecipientCanContinueInPrivateCopyFromTheUi() {
        wakeEmulatorScreen()
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("copy-ui-manager-${System.nanoTime()}.db")
        val recipientDb = context.filesDir.resolve("families.db")
        val manager = NativeLocalStore.open(managerDb.absolutePath).use { local ->
            val family = local.createFamily(System.currentTimeMillis())
            local.addChild(family, "UI private child", System.currentTimeMillis())
            family
        }
        val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(manager, origin, publicKey)
            sharing.invite(manager, origin, 1u.toUByte())
        }
        val recipient = ShareCoordinator(context, recipientDb.absolutePath).use { it.claim(fragment).family }
        ShareCoordinator(context, managerDb.absolutePath).use { assertTrue(it.advanceManager(manager, origin).ready) }
        ShareCoordinator(context, recipientDb.absolutePath).use { assertTrue(it.advanceRecipient(recipient).awaitingGrant) }
        ShareCoordinator(context, managerDb.absolutePath).use { assertTrue(it.advanceManager(manager, origin).ready) }
        ShareCoordinator(context, recipientDb.absolutePath).use { assertTrue(it.advanceRecipient(recipient).ready) }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.removeDevice(manager, origin, recipient.deviceId)
        }
        ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            val removed = sharing.advanceRecipient(recipient)
            assertTrue(removed.removed)
            assertEquals(null, removed.privateCopy)
        }
        val before = NativeLocalStore.open(recipientDb.absolutePath).use { local ->
            local.families().map { it.familyId.joinToString("") { byte -> "%02x".format(byte.toInt() and 255) } }.toSet()
        }
        context.getSharedPreferences("tracker_selection", android.content.Context.MODE_PRIVATE)
            .edit().putString("family", recipient.familyId.hex()).remove("child").commit()
        ActivityScenario.launch(MainActivity::class.java).use {
            val button = context.getString(R.string.continue_in_private_copy)
            val switch = context.getString(R.string.switch_target)
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText(button).fetchSemanticsNodes().isNotEmpty() ||
                    composeRule.onAllNodesWithContentDescription(switch).fetchSemanticsNodes().isNotEmpty()
            }
            if (composeRule.onAllNodesWithContentDescription(switch)
                    .fetchSemanticsNodes().isNotEmpty()) openTab(R.string.nav_family)
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText(button).fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(button).performScrollTo().performClick()
            val deadline = System.currentTimeMillis() + 25_000
            var copied = false
            while (System.currentTimeMillis() < deadline) {
                copied = NativeLocalStore.open(recipientDb.absolutePath).use { local ->
                    local.families().any { family ->
                        val id = family.familyId.joinToString("") { byte -> "%02x".format(byte.toInt() and 255) }
                        id !in before && local.children(family).any { child -> child.name == "UI private child" }
                    }
                }
                if (copied) break
                Thread.sleep(200)
            }
            assertTrue("The recovery action should create a local Family with held history", copied)
        }
    }

    @Test
    fun managerCancelsUnusedInvitationBeforeClaim() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("cancel-manager-${System.nanoTime()}.db")
        val recipientDb = context.filesDir.resolve("cancel-recipient-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(managerDb.absolutePath).use {
            it.createFamily(System.currentTimeMillis())
        }
        val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
        }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            val invitationId = sharing.unusedInvitationIds(family).single()
            val key = DeviceWrappingKey(context).loadOrCreate()
            try {
                val first = NativeSharedStore.open(managerDb.absolutePath).use { core ->
                    core.prepareInviteCancel(family, key, invitationId).candidateBytes
                }
                val resumed = NativeSharedStore.open(managerDb.absolutePath).use { core ->
                    core.prepareInviteCancel(family, key, invitationId).candidateBytes
                }
                assertArrayEquals(first, resumed)
            } finally {
                key.fill(0)
            }
            sharing.cancelInvitation(family, origin, invitationId)
            assertTrue(sharing.unusedInvitationIds(family).isEmpty())
        }
        ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            val result = runCatching { sharing.claim(fragment) }
            val terminal = result.exceptionOrNull() as? InvitationTerminal
            assertEquals(InvitationTerminalReason.CANCELED, terminal?.reason)
            assertTrue(sharing.recipientFamilies().isEmpty())
        }
    }

    @Test
    fun lostCancelResponseDoesNotBlockAnotherInvitation() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("lost-cancel-manager-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(managerDb.absolutePath).use {
            it.createFamily(System.currentTimeMillis())
        }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
            sharing.invite(family, origin, 1u.toUByte())
            val ids = sharing.unusedInvitationIds(family)
            assertEquals(2, ids.size)
            val key = DeviceWrappingKey(context).loadOrCreate()
            val first = try {
                NativeSharedStore.open(managerDb.absolutePath).use { core ->
                    core.prepareInviteCancel(family, key, ids[0]).candidateBytes
                }
            } finally {
                key.fill(0)
            }
            RelayTransport(origin).post(
                "/v1/families/${family.familyId.joinToString("") { "%02x".format(it) }}/control",
                first,
            ) // The signed response is lost before local confirmation.
            sharing.cancelInvitation(family, origin, ids[1])
            assertTrue(sharing.unusedInvitationIds(family).isEmpty())
        }
    }

    @Test
    fun managerCanCancelAnUnusedInvitationFromTheUi() {
        wakeEmulatorScreen()
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val db = context.filesDir.resolve("families.db")
        val family = NativeLocalStore.open(db.absolutePath).use {
            it.createFamily(System.currentTimeMillis())
        }
        val invitationId = ShareCoordinator(context, db.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
            sharing.unusedInvitationIds(family).single()
        }
        context.getSharedPreferences("shared_relay_origins", android.content.Context.MODE_PRIVATE)
            .edit().putString(family.familyId.joinToString("") { "%02x".format(it) }, origin).commit()
        context.getSharedPreferences("tracker_selection", android.content.Context.MODE_PRIVATE)
            .edit().putString("family", family.familyId.joinToString("") { "%02x".format(it) }).commit()
        val shortId = invitationId.joinToString("") { "%02x".format(it) }.take(8)
        ActivityScenario.launch(MainActivity::class.java).use {
            val button = context.getString(R.string.cancel_invitation, shortId)
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText(context.getString(R.string.show_family_access))
                    .fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(context.getString(R.string.show_family_access))
                .performScrollTo().performClick()
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText(button).fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(button).performScrollTo().performClick()
            val confirm = context.getString(R.string.confirm_cancel_invitation)
            composeRule.waitUntil(15_000) {
                composeRule.onAllNodesWithText(confirm).fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(confirm).performClick()
            val committedDeadline = System.currentTimeMillis() + 25_000
            var canceled = false
            while (System.currentTimeMillis() < committedDeadline) {
                canceled = ShareCoordinator(context, db.absolutePath).use { sharing ->
                    sharing.unusedInvitationIds(family).isEmpty()
                }
                if (canceled) break
                Thread.sleep(200)
            }
            assertTrue("The UI action should commit the invitation cancellation", canceled)
        }
    }
    @Test
    fun sharedInvitationOpensJoinFormWithoutRedeemingIt() {
        wakeEmulatorScreen()
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val fragment = "#bt-invite=v1.received-for-review"
        val previousRecipients = ShareCoordinator(
            context,
            context.filesDir.resolve("families.db").absolutePath,
        ).use { it.recipientFamilies().size }
        val intent = Intent(context, MainActivity::class.java).apply {
            action = Intent.ACTION_SEND
            type = "text/plain"
            putExtra(Intent.EXTRA_TEXT, fragment)
        }
        ActivityScenario.launch<MainActivity>(intent).use {
            val deadline = System.currentTimeMillis() + 15_000
            var visible = false
            while (System.currentTimeMillis() < deadline) {
                val root = instrumentation.uiAutomation.rootInActiveWindow
                visible = root?.containsText(fragment) == true
                if (visible) break
                root?.scrollForward()
                Thread.sleep(250)
            }
            assertTrue("Shared invitation should prefill the join form", visible)
            ShareCoordinator(context, context.filesDir.resolve("families.db").absolutePath).use { sharing ->
                assertEquals("Receiving an invitation must not claim it", previousRecipients, sharing.recipientFamilies().size)
            }
        }
        val link = Intent(Intent.ACTION_VIEW, android.net.Uri.parse("babytrack://join$fragment")).apply {
            addCategory(Intent.CATEGORY_BROWSABLE)
            setPackage(context.packageName)
        }
        assertEquals(MainActivity::class.java.name,
            link.resolveActivity(context.packageManager)?.className)
        ActivityScenario.launch<MainActivity>(link).use {
            val deadline = System.currentTimeMillis() + 15_000
            var visible = false
            while (System.currentTimeMillis() < deadline) {
                val root = instrumentation.uiAutomation.rootInActiveWindow
                visible = root?.containsText(fragment) == true
                if (visible) break
                root?.scrollForward()
                Thread.sleep(250)
            }
            assertTrue("Invitation link should prefill the join form", visible)
            ShareCoordinator(context, context.filesDir.resolve("families.db").absolutePath).use { sharing ->
                assertEquals("Opening an invitation link must not claim it",
                    previousRecipients, sharing.recipientFamilies().size)
            }
        }
    }

    @Test
    fun invitationLinkStartsOneActionJoinThroughTheUi() {
        wakeEmulatorScreen()
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("ui-join-manager-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(managerDb.absolutePath).use { local ->
            local.createFamily(System.currentTimeMillis())
        }
        val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
        }
        val link = Intent(Intent.ACTION_VIEW, android.net.Uri.parse("babytrack://join$fragment")).apply {
            addCategory(Intent.CATEGORY_BROWSABLE)
            setPackage(context.packageName)
        }
        ActivityScenario.launch<MainActivity>(link).use { scenario ->
            val label = context.getString(R.string.join_or_retry)
            composeRule.waitUntil(25_000) {
                runCatching {
                    composeRule.onNodeWithText(label).performScrollTo().assertIsDisplayed()
                }.isSuccess
            }
            composeRule.onNodeWithText(label).assertIsDisplayed().performClick()

            val saved = context.filesDir.resolve("families.db").absolutePath
            val claimDeadline = System.currentTimeMillis() + 45_000
            var committed = false
            var joiningIndex = -1
            while (System.currentTimeMillis() < claimDeadline) {
                val key = DeviceWrappingKey(context).loadOrCreate()
                committed = try {
                    NativeSharedStore.open(saved).use { core ->
                        val recipients = core.recipientFamilies().filterNot { core.isRemoved(it) }
                        joiningIndex = recipients.indexOfFirst { it.familyId.contentEquals(family.familyId) }
                        recipients.getOrNull(joiningIndex)
                            ?.let { core.recipientFirstJoinAction(it, key) == 0u.toUByte() } ?: false
                    }
                } catch (failure: Exception) {
                    // The UI coordinator can hold the SQLite writer while this
                    // independent assertion connection reads its claim state.
                    // Retry only that transient lock; other failures are real.
                    if (!failure.message.orEmpty().contains("DatabaseBusy")) throw failure
                    false
                } finally {
                    key.fill(0)
                }
                if (committed) break
                Thread.sleep(200)
            }
            assertTrue("The single UI action should commit a saved recipient claim", committed)
            val joining = context.getString(R.string.joining_family_number, joiningIndex + 1)
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText(joining).fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(joining).performScrollTo().assertIsDisplayed().performClick()
            scenario.recreate()
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText(joining).fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(joining).performScrollTo().assertIsDisplayed().performClick()
            composeRule.onNodeWithText(context.getString(R.string.saved_join_pending))
                .performScrollTo().assertIsDisplayed()
            assertEquals(0, composeRule.onAllNodesWithText(fragment).fetchSemanticsNodes().size)
        }
    }

    @Test
    fun activityRecreationResumesSavedRecipientClaim() {
        wakeEmulatorScreen()
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("activity-manager-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(managerDb.absolutePath).use { it.createFamily(System.currentTimeMillis()) }
        val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
        }
        val preview = previewInvitation(fragment)
        val wrapping = DeviceWrappingKey(context).loadOrCreate()
        val recipient = try {
            NativeSharedStore.open(context.filesDir.resolve("families.db").absolutePath).use { core ->
                core.prepareJoin(fragment, RelayTransport(origin).get(preview.controlPath, preview.readAuth), wrapping).family
            }
        } finally {
            wrapping.fill(0)
        }
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            scenario.recreate()
            val deadline = System.currentTimeMillis() + 12_000
            var resumed = false
            var visible = false
            while (System.currentTimeMillis() < deadline) {
                val root = instrumentation.uiAutomation.rootInActiveWindow
                visible = root?.packageName?.toString() == context.packageName
                val wrappingAfter = DeviceWrappingKey(context).loadOrCreate()
                resumed = try {
                    NativeSharedStore.open(context.filesDir.resolve("families.db").absolutePath).use { core ->
                        core.recipientFirstJoinAction(recipient, wrappingAfter) == 0u.toUByte()
                    }
                } catch (failure: Exception) {
                    // The recreated app can write the saved join state while
                    // this independent assertion connection reads it. Retry
                    // only that transient lock within the existing deadline.
                    if (!failure.message.orEmpty().contains("DatabaseBusy")) throw failure
                    false
                } finally {
                    wrappingAfter.fill(0)
                }
                if (visible && resumed) break
                Thread.sleep(100)
            }
            assertTrue("Recreated UI should resume and confirm the saved claim (visible=$visible, resumed=$resumed)", visible && resumed)
        }
        ShareCoordinator(context, context.filesDir.resolve("families.db").absolutePath).use { sharing ->
            assertTrue(sharing.recipientFamilies().any { it.familyId.contentEquals(recipient.familyId) })
        }
    }

    private fun AccessibilityNodeInfo.containsText(text: String): Boolean {
        if (this.text?.toString()?.contains(text) == true) return true
        return (0 until childCount).any { index -> getChild(index)?.containsText(text) == true }
    }

    private fun AccessibilityNodeInfo.scrollForward(): Boolean {
        if (isScrollable && performAction(AccessibilityNodeInfo.ACTION_SCROLL_FORWARD)) return true
        return (0 until childCount).any { index -> getChild(index)?.scrollForward() == true }
    }

    private fun wakeEmulatorScreen() {
        val automation = InstrumentationRegistry.getInstrumentation().uiAutomation
        for (command in listOf("input keyevent KEYCODE_WAKEUP", "wm dismiss-keyguard")) {
            ParcelFileDescriptor.AutoCloseInputStream(automation.executeShellCommand(command))
                .bufferedReader().use { it.readText() }
        }
    }

    @Test
    fun savedClaimRetriesByFamilyAfterPrecommitFailureOrLostResponse() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        for (committedBeforeRestart in listOf(false, true)) {
            val managerDb = context.filesDir.resolve("claim-manager-${System.nanoTime()}.db")
            val recipientDb = context.filesDir.resolve("claim-recipient-${System.nanoTime()}.db")
            val family = NativeLocalStore.open(managerDb.absolutePath).use { local ->
                local.createFamily(System.currentTimeMillis())
            }
            val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
                sharing.promote(family, origin, publicKey)
                sharing.invite(family, origin, 1u.toUByte())
            }
            val preview = previewInvitation(fragment)
            val relay = RelayTransport(origin)
            val wrapping = DeviceWrappingKey(context).loadOrCreate()
            val prepared = try {
                NativeSharedStore.open(recipientDb.absolutePath).use { core ->
                    val page = relay.get(preview.controlPath, preview.readAuth)
                    core.prepareJoin(fragment, page, wrapping)
                }
            } finally {
                wrapping.fill(0)
            }
            if (committedBeforeRestart) {
                relay.post("/v1/families/${family.familyId.joinToString("") { "%02x".format(it) }}/control", prepared.candidateBytes)
                // Drop the signed response before the local claim is confirmed.
            }
            ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
                assertArrayEquals(prepared.family.familyId, sharing.recipientFamilies().single().familyId)
                assertEquals(origin, sharing.recipientOrigin(prepared.family))
                assertTrue(sharing.advanceRecipient(prepared.family).awaitingGrant)
                assertArrayEquals(prepared.candidateBytes, sharing.retryClaim(prepared.family).candidateBytes)
            }
        }
    }

    @Test
    fun scheduledJobRetriesSavedRecipientClaim() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("job-claim-manager-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(managerDb.absolutePath).use { it.createFamily(System.currentTimeMillis()) }
        val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
        }
        val preview = previewInvitation(fragment)
        val wrapping = DeviceWrappingKey(context).loadOrCreate()
        val recipient = try {
            NativeSharedStore.open(context.filesDir.resolve("families.db").absolutePath).use { core ->
                core.prepareJoin(fragment, RelayTransport(origin).get(preview.controlPath, preview.readAuth), wrapping).family
            }
        } finally {
            wrapping.fill(0)
        }
        SharedSyncJobService.schedule(context)
        val before = context.getSharedPreferences("shared_background_sync", android.content.Context.MODE_PRIVATE)
            .getLong("last_attempt_ms", 0)
        val command = instrumentation.uiAutomation.executeShellCommand("cmd jobscheduler run -f org.babytrack.app 3400")
        ParcelFileDescriptor.AutoCloseInputStream(command).bufferedReader().use { it.readText() }
        val deadline = System.currentTimeMillis() + 15_000
        var resumed = false
        while (System.currentTimeMillis() < deadline) {
            val newAttempt = context.getSharedPreferences("shared_background_sync", android.content.Context.MODE_PRIVATE)
                .getLong("last_attempt_ms", 0) > before
            if (newAttempt) {
                val key = DeviceWrappingKey(context).loadOrCreate()
                resumed = try {
                    NativeSharedStore.open(context.filesDir.resolve("families.db").absolutePath).use { core ->
                        core.recipientFirstJoinAction(recipient, key) == 0u.toUByte()
                    }
                } finally {
                    key.fill(0)
                }
            }
            if (newAttempt && resumed) break
            Thread.sleep(100)
        }
        assertTrue("Background job should confirm the saved claim", resumed)
    }

    @Test
    fun scheduledJobUploadsSavedManagerEdit() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val database = context.filesDir.resolve("families.db")
        val family = NativeLocalStore.open(database.absolutePath).use { local ->
            local.createFamily(System.currentTimeMillis())
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.addChild(family, "Background child", System.currentTimeMillis())
            assertEquals(1uL, sharing.snapshot(family).unsentCount)
        }
        context.getSharedPreferences("shared_relay_origins", android.content.Context.MODE_PRIVATE)
            .edit().putString(family.familyId.joinToString("") { "%02x".format(it) }, origin).commit()
        SharedSyncJobService.schedule(context)
        assertTrue(context.getSystemService(JobScheduler::class.java).getPendingJob(3400) != null)
        val before = context.getSharedPreferences("shared_background_sync", android.content.Context.MODE_PRIVATE)
            .getLong("last_attempt_ms", 0)
        val command = instrumentation.uiAutomation.executeShellCommand("cmd jobscheduler run -f org.babytrack.app 3400")
        ParcelFileDescriptor.AutoCloseInputStream(command).bufferedReader().use { it.readText() }
        val deadline = System.currentTimeMillis() + 15_000
        while (System.currentTimeMillis() < deadline) {
            val done = context.getSharedPreferences("shared_background_sync", android.content.Context.MODE_PRIVATE)
                .getLong("last_attempt_ms", 0) > before
            if (done) break
            Thread.sleep(100)
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            assertEquals(0uL, sharing.snapshot(family).unsentCount)
        }
    }

    @Test
    fun backgroundRemovalShowsExactSavedCopyAfterReopen() {
        wakeEmulatorScreen()
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val database = context.filesDir.resolve("families.db")
        val acknowledgments = context.getSharedPreferences(
            "acknowledged_removal_copies", android.content.Context.MODE_PRIVATE,
        )
        val original = NativeLocalStore.open(database.absolutePath).use { local ->
            val created = local.createFamily(System.currentTimeMillis())
            local.createFamily(System.currentTimeMillis() + 1) // another selectable Family
            created
        }
        val holderDb = context.filesDir.resolve("background-removal-holder-${System.nanoTime()}.db")
        val fragment = ShareCoordinator(context, database.absolutePath).use { sharing ->
            sharing.promote(original, origin, publicKey)
            sharing.invite(original, origin, 2u.toUByte())
        }
        val holder = ShareCoordinator(context, holderDb.absolutePath).use { it.claim(fragment).family }
        ShareCoordinator(context, database.absolutePath).use { assertTrue(it.advanceManager(original, origin).ready) }
        ShareCoordinator(context, holderDb.absolutePath).use { assertTrue(it.advanceRecipient(holder).awaitingGrant) }
        ShareCoordinator(context, database.absolutePath).use { assertTrue(it.advanceManager(original, origin).ready) }
        ShareCoordinator(context, holderDb.absolutePath).use { assertTrue(it.advanceRecipient(holder).ready) }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            sharing.addChild(original, "Saved while offline", System.currentTimeMillis())
            assertEquals(1uL, sharing.snapshot(original).unsentCount)
        }
        ShareCoordinator(context, holderDb.absolutePath).use { sharing ->
            sharing.invite(holder, origin, 1u.toUByte())
            sharing.removeDevice(holder, origin, original.deviceId)
        }
        context.getSharedPreferences("shared_relay_origins", android.content.Context.MODE_PRIVATE)
            .edit().putString(original.familyId.hex(), origin).commit()
        SharedSyncJobService.schedule(context)
        val jobStatus = context.getSharedPreferences("shared_background_sync", android.content.Context.MODE_PRIVATE)
        val before = jobStatus.getLong("last_attempt_ms", 0)
        val command = instrumentation.uiAutomation.executeShellCommand("cmd jobscheduler run -f org.babytrack.app 3400")
        ParcelFileDescriptor.AutoCloseInputStream(command).bufferedReader().use { it.readText() }
        val deadline = System.currentTimeMillis() + 25_000
        var copy: uniffi.babytrack_core_ffi.FamilyRef? = null
        while (System.currentTimeMillis() < deadline) {
            ShareCoordinator(context, database.absolutePath).use { sharing ->
                copy = sharing.savedRemovalCopy(original)
            }
            if (copy != null && jobStatus.getLong("last_attempt_ms", 0) > before) break
            Thread.sleep(100)
        }
        val saved = copy ?: error("Background removal did not save pending work")
        NativeLocalStore.open(database.absolutePath).use { local ->
            assertTrue(local.children(saved).any { it.name == "Saved while offline" })
        }
        val noticeText = context.getString(
            R.string.history_removed_copy_destination,
            original.familyId.hex().take(8), saved.familyId.hex().take(8),
        )
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            scenario.recreate()
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText(noticeText).fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(noticeText).assertIsDisplayed()
            composeRule.onAllNodesWithText(context.getString(R.string.continue_in_private_copy))
                .onLast().performClick()
            composeRule.waitUntil(15_000) {
                context.getSharedPreferences("tracker_selection", android.content.Context.MODE_PRIVATE)
                    .getString("family", null) == saved.familyId.hex() &&
                    acknowledgments.getString(original.familyId.hex(), null) == saved.familyId.hex()
            }
            scenario.recreate()
            composeRule.waitUntil(15_000) {
                composeRule.onAllNodesWithText(noticeText).fetchSemanticsNodes().isEmpty()
            }
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            assertArrayEquals(saved.familyId, sharing.savedRemovalCopy(original)!!.familyId)
        }
    }

    @Test
    fun asynchronousJoinAdvancesWithoutManualHolderApproval() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("auto-manager-${System.nanoTime()}.db")
        val recipientDb = context.filesDir.resolve("auto-recipient-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(managerDb.absolutePath).use { local ->
            val created = local.createFamily(System.currentTimeMillis())
            local.addChild(created, "Async child", System.currentTimeMillis())
            created
        }
        val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
        }
        val recipient = ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            sharing.claim(fragment).family
        }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceManager(family, origin).ready)
        }
        ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceRecipient(recipient).awaitingGrant)
        }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceManager(family, origin).ready)
        }
        ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceRecipient(recipient).ready)
            assertEquals("Async child", sharing.snapshot(recipient).children.single().name)
        }
    }

    @Test
    fun admittedManagerInvitesAndGrantsAnotherDevice() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val suffix = System.nanoTime()
        val managerDb = context.filesDir.resolve("handoff-manager-$suffix.db")
        val holderDb = context.filesDir.resolve("handoff-holder-$suffix.db")
        val thirdDb = context.filesDir.resolve("handoff-third-$suffix.db")
        val manager = NativeLocalStore.open(managerDb.absolutePath).use { local ->
            val family = local.createFamily(System.currentTimeMillis())
            local.addChild(family, "Handoff child", System.currentTimeMillis())
            family
        }
        val holderLink = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(manager, origin, publicKey)
            sharing.invite(manager, origin, 2u.toUByte())
        }
        val holder = ShareCoordinator(context, holderDb.absolutePath).use { it.claim(holderLink).family }
        ShareCoordinator(context, managerDb.absolutePath).use { assertTrue(it.advanceManager(manager, origin).ready) }
        ShareCoordinator(context, holderDb.absolutePath).use { assertTrue(it.advanceRecipient(holder).awaitingGrant) }
        ShareCoordinator(context, managerDb.absolutePath).use { assertTrue(it.advanceManager(manager, origin).ready) }
        val thirdLink = ShareCoordinator(context, holderDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceRecipient(holder).ready)
            assertTrue(sharing.isAdmittedManager(holder))
            sharing.invite(holder, origin, 1u.toUByte())
        }
        NativeLocalStore.open(thirdDb.absolutePath).use { it.createFamily(System.currentTimeMillis()) }
        val third = ShareCoordinator(context, thirdDb.absolutePath).use { it.claim(thirdLink).family }
        NativeLocalStore.open(thirdDb.absolutePath).use { local ->
            ShareCoordinator(context, thirdDb.absolutePath).use { sharing ->
                assertEquals(1, loadTrackerData(local, sharing, null, null, null).families.size)
            }
        }
        ShareCoordinator(context, holderDb.absolutePath).use { assertTrue(it.advanceRecipient(holder).ready) }
        ShareCoordinator(context, thirdDb.absolutePath).use { assertTrue(it.advanceRecipient(third).awaitingGrant) }
        ShareCoordinator(context, holderDb.absolutePath).use { assertTrue(it.advanceRecipient(holder).ready) }
        ShareCoordinator(context, thirdDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceRecipient(third).ready)
            assertEquals("Handoff child", sharing.snapshot(third).children.single().name)
            NativeLocalStore.open(thirdDb.absolutePath).use { local ->
                val selected = third.familyId.joinToString("") { "%02x".format(it) }
                val tracker = loadTrackerData(local, sharing, selected, null, null)
                assertEquals(2, tracker.families.size)
                assertEquals(selected, tracker.activeFamilyKey)
                assertEquals("Handoff child", tracker.familyChildNames[selected])
                assertTrue(tracker.shared)
                assertTrue(!tracker.activeFamilyIsLocal)
                assertEquals("Handoff child", tracker.children.single().name)
            }
        }
        ShareCoordinator(context, holderDb.absolutePath).use { sharing ->
            sharing.addChild(holder, "Holder child", System.currentTimeMillis())
            assertTrue(sharing.syncRecipientAndUpload(holder).ready)
        }
        ShareCoordinator(context, thirdDb.absolutePath).use { sharing ->
            assertTrue(sharing.syncRecipientAndUpload(third).ready)
            assertTrue(sharing.snapshot(third).children.any { it.name == "Holder child" })
        }
        val fourthDb = context.filesDir.resolve("handoff-fourth-$suffix.db")
        val fourthLink = ShareCoordinator(context, holderDb.absolutePath).use { sharing ->
            sharing.invite(holder, origin, 1u.toUByte())
        }
        val fourth = ShareCoordinator(context, fourthDb.absolutePath).use { it.claim(fourthLink).family }
        ShareCoordinator(context, holderDb.absolutePath).use { sharing ->
            assertTrue(sharing.syncRecipientAndUpload(holder).ready)
            val pendingFourth = sharing.snapshot(holder).pendingDevices.single {
                it.deviceId.contentEquals(fourth.deviceId)
            }
            val after = sharing.removePendingDevice(
                holder, origin, pendingFourth.invitationId, fourth.deviceId,
            )
            assertTrue(after.pendingDevices.none { it.deviceId.contentEquals(fourth.deviceId) })
        }
        ShareCoordinator(context, fourthDb.absolutePath).use { sharing ->
            assertEquals(8u.toUByte(), sharing.advanceRecipient(fourth).joinPhase)
        }
        val unusedLink = ShareCoordinator(context, holderDb.absolutePath).use { sharing ->
            val link = sharing.invite(holder, origin, 1u.toUByte())
            sharing.cancelInvitation(holder, origin, sharing.unusedInvitationIds(holder).single())
            link
        }
        val canceledDb = context.filesDir.resolve("handoff-canceled-$suffix.db")
        ShareCoordinator(context, canceledDb.absolutePath).use { sharing ->
            assertEquals(InvitationTerminalReason.CANCELED,
                (runCatching { sharing.claim(unusedLink) }.exceptionOrNull() as? InvitationTerminal)?.reason)
        }
        ShareCoordinator(context, holderDb.absolutePath).use { sharing ->
            val promoted = sharing.changeDeviceRole(holder, origin, third.deviceId, 2u.toUByte())
            assertEquals(2u.toUByte(), promoted.devices.single {
                it.deviceId.contentEquals(third.deviceId)
            }.role)
        }
        ShareCoordinator(context, thirdDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceRecipient(third).ready)
            assertTrue(sharing.isAdmittedManager(third))
        }
        ShareCoordinator(context, holderDb.absolutePath).use { sharing ->
            val demoted = sharing.changeDeviceRole(holder, origin, third.deviceId, 1u.toUByte())
            assertEquals(1u.toUByte(), demoted.devices.single {
                it.deviceId.contentEquals(third.deviceId)
            }.role)
        }
        ShareCoordinator(context, thirdDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceRecipient(third).ready)
            assertTrue(!sharing.isAdmittedManager(third))
            assertTrue(runCatching { sharing.invite(third, origin, 1u.toUByte()) }.isFailure)
        }
        ShareCoordinator(context, holderDb.absolutePath).use { sharing ->
            val rotated = sharing.removeDevice(holder, origin, third.deviceId)
            assertEquals(2, rotated.devices.size)
            assertTrue(rotated.devices.none { it.deviceId.contentEquals(third.deviceId) })
        }
        ShareCoordinator(context, thirdDb.absolutePath).use { sharing ->
            assertTrue(sharing.syncRecipient(third).removed)
            assertTrue(runCatching { sharing.snapshot(third) }.isFailure)
            val copy = sharing.privateCopy(third, System.currentTimeMillis())
            NativeLocalStore.open(thirdDb.absolutePath).use { local ->
                assertTrue(local.children(copy).any { it.name == "Holder child" })
                val screen = loadTrackerData(local, sharing,
                    third.familyId.joinToString("") { "%02x".format(it) }, null,
                    third.familyId.joinToString("") { "%02x".format(it) })
                assertTrue(screen.removedFamilies.any { it.familyId.contentEquals(third.familyId) })
                assertTrue(screen.families.none { it.familyId.contentEquals(third.familyId) })
                assertTrue(screen.recipients.none { it.familyId.contentEquals(third.familyId) })
                assertTrue(screen.families.any { it.familyId.contentEquals(copy.familyId) })
            }
        }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            assertTrue(sharing.syncAndUpload(manager, origin).ready)
            assertTrue(sharing.snapshot(manager).devices.none { it.deviceId.contentEquals(third.deviceId) })
            sharing.addChild(manager, "Manager offline before removal", System.currentTimeMillis())
        }
        ShareCoordinator(context, holderDb.absolutePath).use { sharing ->
            val rotatedAgain = sharing.removeDevice(holder, origin, manager.deviceId)
            assertEquals(1, rotatedAgain.devices.size)
            assertArrayEquals(holder.deviceId, rotatedAgain.devices.single().deviceId)
            sharing.addChild(holder, "After second rotation", System.currentTimeMillis())
            assertTrue(sharing.syncRecipientAndUpload(holder).ready)
            assertTrue(sharing.snapshot(holder).children.any { it.name == "After second rotation" })
        }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            val removed = sharing.checkInitialManagerRemoval(manager, origin)
                ?: error("Original manager should verify its removal")
            val copy = removed.privateCopy ?: error("Offline work needs a private copy")
            assertTrue(runCatching { sharing.snapshot(manager) }.isFailure)
            NativeLocalStore.open(managerDb.absolutePath).use { local ->
                assertTrue(local.children(copy).any { it.name == "Manager offline before removal" })
                val screen = loadTrackerData(local, sharing,
                    manager.familyId.joinToString("") { "%02x".format(it) }, null, null)
                assertTrue(screen.removedFamilies.any { it.familyId.contentEquals(manager.familyId) })
                assertTrue(screen.families.none { it.familyId.contentEquals(manager.familyId) })
                assertTrue(screen.families.any { it.familyId.contentEquals(copy.familyId) })
                assertArrayEquals(copy.familyId,
                    sharing.privateCopy(manager, System.currentTimeMillis()).familyId)
            }
        }
    }

    @Test
    fun keystoreWrappedPromotionAndInvitationSurviveRestart() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        if (android.os.Build.VERSION.SDK_INT >= 33)
            instrumentation.uiAutomation.grantRuntimePermission(context.packageName, Manifest.permission.POST_NOTIFICATIONS)
        val timerDrafts = liveTimerStore(context)
        val notifications = context.getSystemService(NotificationManager::class.java)
        lateinit var notificationSession: TimerNotificationTarget
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
        val fragment = ShareCoordinator(context, database.absolutePath).use { sharing ->
            assertEquals(1uL, sharing.promote(family, origin, publicKey))
            assertEquals("Relay test child", sharing.snapshot(family).children.single().name)
            assertEquals(1, sharing.snapshot(family).devices.size)
            NativeLocalStore.open(database.absolutePath).use { local ->
                assertTrue(runCatching { local.addChild(family, "Wrong surface", System.currentTimeMillis()) }.isFailure)
            }
            val fragment = sharing.invite(family, origin, 1u.toUByte())
            assertTrue(fragment.startsWith("#bt-invite=v1."))
            fragment
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            assertEquals(2uL, sharing.promote(family, origin, publicKey))
            assertTrue(sharing.invite(family, origin, 1u.toUByte()).startsWith("#bt-invite=v1."))
        }
        val recipient = context.filesDir.resolve("recipient-test-${System.nanoTime()}.db")
        val first = ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            sharing.claim(fragment)
        }
        assertArrayEquals(family.familyId, first.family.familyId)
        val retried = ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            sharing.claim(fragment)
        }
        assertEquals(first.family.deviceId.toList(), retried.family.deviceId.toList())
        assertArrayEquals(first.candidateBytes, retried.candidateBytes)
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            assertArrayEquals(first.family.familyId, sharing.recipientFamilies().single().familyId)
            assertEquals(origin, sharing.recipientOrigin(first.family))
        }
        NativeLocalStore.open(recipient.absolutePath).use { local ->
            assertTrue(local.families().none { it.familyId.contentEquals(family.familyId) })
            assertTrue(
                runCatching {
                    local.addChild(first.family, "Too early", System.currentTimeMillis())
                }.isFailure,
            )
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            sharing.respondToClaim(family, origin)
        }
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            sharing.proveChallenge(first.family)
        }
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            val pending = sharing.syncRecipient(first.family)
            assertTrue(pending.awaitingGrant)
            assertTrue(!pending.ready)
            assertTrue(runCatching { sharing.snapshot(first.family) }.isFailure)
            assertTrue(runCatching { sharing.addChild(first.family, "Too early", System.currentTimeMillis()) }.isFailure)
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            sharing.admitProvedDevice(family, origin)
        }
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            val synced = sharing.syncRecipient(first.family)
            assertTrue(synced.ready)
            assertEquals(1uL, synced.childCount)
            val existing = sharing.snapshot(first.family).children.single()
            assertEquals("Relay test child", existing.name)
            val timerTarget = timerTarget(first.family.familyId.key(), existing.id.key())
            timerDrafts.savePumpStart(timerTarget, System.currentTimeMillis() - 60_000L)
            notificationSession = timerDrafts.session(timerTarget, LiveTimerKind.PUMP)!!
            NativeLocalStore.open(recipient.absolutePath).use { local ->
                refreshLiveTimerNotifications(context, local, sharing)
            }
            composeRule.waitUntil(10_000) {
                notifications.activeNotifications.any { it.tag == LiveTimerNotifications.tag(notificationSession) }
            }
            assertEquals(context.getString(R.string.pump_notification_title, existing.name),
                notifications.activeNotifications.single { it.tag == LiveTimerNotifications.tag(notificationSession) }
                    .notification.extras.getString("android.title"))
            assertEquals(2, sharing.snapshot(first.family).devices.size)
            assertTrue(sharing.snapshot(first.family).devices.any {
                it.deviceId.contentEquals(first.family.deviceId) && it.role == 1u.toUByte()
            })
            sharing.addChild(first.family, "Offline shared child", System.currentTimeMillis())
            val now = System.currentTimeMillis()
            assertTrue(runCatching {
                sharing.logDiaper(first.family, ByteArray(16), 1u.toUByte(), ActivityWhen(now, 0, now))
            }.isFailure)
            sharing.logDiaper(first.family, existing.id, 1u.toUByte(), ActivityWhen(now, 0, now))
            assertEquals(2, sharing.snapshot(first.family).children.size)
            assertEquals(1, sharing.snapshot(first.family).activities.size)
            assertEquals(2uL, sharing.snapshot(first.family).unsentCount)
            assertEquals(0uL, sharing.snapshot(first.family).inertCount)
            val file = sharing.backupFile(first.family, now, null, 512_000_000uL)
            assertArrayEquals(first.family.familyId, file.info.sourceFamilyId)
            assertEquals(4uL, file.info.recordCount)
            val protected = sharing.backupFile(first.family, now, "backup secret", 512_000_000uL)
            NativeLocalStore.open(context.filesDir.resolve("restored-${System.nanoTime()}.db").absolutePath).use { local ->
                assertEquals(4uL, local.inspectProtected(protected.bytes, "backup secret", 512_000_000uL).recordCount)
                assertTrue(runCatching {
                    local.inspectProtected(protected.bytes, "wrong secret", 512_000_000uL)
                }.isFailure)
                val restored = local.restore(file.bytes, now)
                assertEquals(2, local.children(restored).size)
            }
            val copy = sharing.privateCopy(first.family, now)
            assertArrayEquals(copy.familyId, sharing.privateCopy(first.family, now + 1).familyId)
            assertTrue(!sharing.isShared(copy))
            NativeLocalStore.open(recipient.absolutePath).use { local ->
                assertEquals(2, local.children(copy).size)
            }
        }
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            assertTrue(sharing.syncRecipient(first.family).ready)
            assertEquals(2, sharing.snapshot(first.family).children.size)
            assertEquals(1, sharing.snapshot(first.family).activities.size)
            val wrapping = DeviceWrappingKey(context).loadOrCreate()
            try {
                NativeSharedStore.open(recipient.absolutePath).use { core ->
                    val candidate = core.prepareSharedUpload(first.family, wrapping)
                        ?: error("Expected an offline batch")
                    val familyHex = first.family.familyId.joinToString("") { "%02x".format(it.toInt() and 255) }
                    RelayTransport(origin).post("/v1/families/$familyHex/batches", candidate)
                    // Drop the response: the next sync must discover its signed acceptance.
                }
            } finally {
                wrapping.fill(0)
            }
            val uploaded = sharing.syncAndUpload(first.family, origin)
            assertTrue(uploaded.ready)
            assertEquals(0u.toUByte(), uploaded.outboxState)
            assertEquals(0uL, sharing.snapshot(first.family).unsentCount)
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            assertTrue(sharing.syncAndUpload(family, origin).ready)
            assertEquals(2, sharing.snapshot(family).children.size)
            assertEquals(1, sharing.snapshot(family).activities.size)
            sharing.addChild(family, "Manager later", System.currentTimeMillis())
            assertTrue(sharing.syncAndUpload(family, origin).ready)
        }
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            assertTrue(sharing.syncAndUpload(first.family, origin).ready)
            assertEquals(3, sharing.snapshot(first.family).children.size)
            sharing.addChild(first.family, "Saved at removal", System.currentTimeMillis())
            assertEquals(1uL, sharing.snapshot(first.family).unsentCount)
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            sharing.admitProvedDevice(family, origin)
        }
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            sharing.proveChallenge(fragment)
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            sharing.respondToClaim(family, origin)
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            val rotated = sharing.removeDevice(family, origin, first.family.deviceId)
            assertEquals(1, rotated.devices.size)
            assertTrue(rotated.devices.single().deviceId.contentEquals(family.deviceId))
            val child = rotated.children.first()
            val now = System.currentTimeMillis()
            sharing.logDiaper(family, child.id, 2u.toUByte(), ActivityWhen(now, 0, now))
            assertTrue(sharing.syncAndUpload(family, origin).ready)
            assertEquals(0uL, sharing.snapshot(family).unsentCount)
        }
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            val removed = sharing.advanceRecipient(first.family)
            assertTrue(removed.removed)
            val copy = removed.privateCopy ?: error("Pending edit needs an independent copy")
            assertTrue(!sharing.isShared(copy))
            NativeLocalStore.open(recipient.absolutePath).use { local ->
                assertTrue(local.children(copy).any { it.name == "Saved at removal" })
            }
            assertArrayEquals(copy.familyId, sharing.advanceRecipient(first.family).privateCopy!!.familyId)
            assertTrue(runCatching { sharing.syncAndUpload(first.family, origin) }.isFailure)
            NativeLocalStore.open(recipient.absolutePath).use { local ->
                refreshLiveTimerNotifications(context, local, sharing)
            }
            composeRule.waitUntil(10_000) {
                notifications.activeNotifications.none { it.tag == LiveTimerNotifications.tag(notificationSession) }
            }
            assertTrue("Removal must retain the original target's local draft", timerDrafts.matches(notificationSession))
            timerDrafts.savePumpStart(notificationSession.target, null)
        }
    }
}
