package org.babytrack.app

import android.view.View
import android.widget.DatePicker
import android.widget.TimePicker
import androidx.test.espresso.Espresso.onView
import androidx.test.espresso.UiController
import androidx.test.espresso.ViewAction
import androidx.test.espresso.action.ViewActions.click
import androidx.test.espresso.matcher.ViewMatchers.isAssignableFrom
import androidx.test.espresso.matcher.ViewMatchers.withId
import org.hamcrest.Matcher
import java.time.LocalDateTime
import java.time.ZoneId
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import java.lang.reflect.Proxy
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.ext.junit.runners.AndroidJUnit4
import uniffi.babytrack_core_ffi.BreastSegmentRow
import uniffi.babytrack_core_ffi.NativeLocalStore
import uniffi.babytrack_core_ffi.FamilyRef

@RunWith(AndroidJUnit4::class)
class BreastEditTest {
    @get:Rule val compose = createComposeRule()
    private val edits = EntryEditState()
    private val feedback = TrackerFeedback()
    private val start = System.currentTimeMillis() - 30 * 60_000L
    private var family = FamilyRef(ByteArray(16) { 1 }, ByteArray(16) { 2 })
    private var child = ByteArray(16) { 3 }
    private var activity = ByteArray(16) { 4 }
    @Volatile private var calls = 0
    private var submitted: List<BreastSegmentRow>? = null
    private var writeThrough: TrackingActions? = null
    @Volatile private var failSave = false
    private var holdSave: CountDownLatch? = null

    private fun show(shared: Boolean = false, recent: Boolean = false) {
        val originalStart = if (recent) System.currentTimeMillis() - 3 * 60_000L - 30_000L else start
        edits.pendingBreastEdit = PendingBreastEdit(
            family, child, activity, shared, originalStart,
            120, listOf(1u.toUByte() to "2", 2u.toUByte() to "1"), listOf(0L, 30_000L),
            finishUtcMs = originalStart + 3 * 60_000L + 30_000L,
        )
        fun adapter(expectedShared: Boolean): TrackingActions = Proxy.newProxyInstance(
            TrackingActions::class.java.classLoader, arrayOf(TrackingActions::class.java),
        ) { _, method, args ->
            check(method.name == "editBreastFeedSegments")
            assertEquals(shared, expectedShared)
            assertSame(family, args[0])
            assertArrayEquals(child, args[1] as ByteArray)
            assertArrayEquals(activity, args[2] as ByteArray)
            @Suppress("UNCHECKED_CAST")
            val segments = args[3] as List<BreastSegmentRow>
            calls++
            holdSave?.let { check(it.await(10, TimeUnit.SECONDS)) }
            check(!failSave) { "Storage unavailable" }
            check(segments.last().endUtcMs <= args[4] as Long) { "Future end" }
            writeThrough?.editBreastFeedSegments(family, child, activity, segments, args[4] as Long)
            submitted = segments
            Unit
        } as TrackingActions
        val router = TrackingActionRouter(adapter(false), adapter(true))
        compose.setContent {
            BabytrackTheme {
                EntryEditController(edits, router, rememberCoroutineScope(), feedback, "Save failed") { saved, action ->
                    runCatching(action).onSuccess { saved?.invoke() }.onFailure { feedback.message = "Save failed" }
                }
            }
        }
    }

    private fun changeFirstMinutes(value: String) {
        compose.onAllNodes(hasSetTextAction())[0].performTextReplacement(value)
    }

    @Test fun increasingOlderLocalFeedSavesSameTargetAndPreservesPause() = validEdit(false)
    @Test fun increasingOlderSharedFeedUsesSharedWriter() = validEdit(true)

    private fun validEdit(shared: Boolean) {
        show(shared)
        changeFirstMinutes("3")
        compose.onNodeWithText("Save changes").performClick()
        compose.waitUntil(5_000) { edits.pendingBreastEdit == null }
        assertEquals(1, calls)
        val rows = submitted!!
        assertEquals(start - 60_000L, rows[0].startUtcMs)
        assertEquals(start + 2 * 60_000L, rows[0].endUtcMs)
        assertEquals(30_000L, rows[1].startUtcMs - rows[0].endUtcMs)
        assertEquals(60_000L, rows[1].endUtcMs - rows[1].startUtcMs)
        assertEquals((java.util.TimeZone.getDefault().getOffset(start - 60_000L) / 60_000).toShort(), rows[0].startOffsetMinutes)
    }

    @Test fun durationIncreasePersistsThroughRustAndRepositoryReopen() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val path = context.cacheDir.resolve("breast-edit-${System.nanoTime()}.db").absolutePath
        NativeLocalStore.open(path).use { store ->
            family = store.createFamily(start)
            child = store.addChild(family, "Breast edit test", start)
            activity = store.logBreastFeedSegments(
                family, child,
                listOf(
                    BreastSegmentRow(1u, start, start + 2 * 60_000L, 120, 120),
                    BreastSegmentRow(2u, start + 2 * 60_000L + 30_000L, start + 3 * 60_000L + 30_000L, 120, 120),
                ),
                uniffi.babytrack_core_ffi.ActivityWhen(start, 120, System.currentTimeMillis()),
            )
            writeThrough = NativeTrackingActions(store)
            show()
            changeFirstMinutes("3")
            compose.onNodeWithText("Save changes").performClick()
            compose.waitUntil(5_000) { edits.pendingBreastEdit == null }
        }
        NativeLocalStore.open(path).use { reopened ->
            val row = reopened.timeline(family, child).single()
            assertArrayEquals(activity, row.id)
            assertEquals(start - 60_000L, row.startUtcMs)
            assertEquals(start + 3 * 60_000L + 30_000L, row.endUtcMs)
            assertEquals(3 * 60_000L, row.breastSegments!![0].endUtcMs - row.startUtcMs)
        }
    }

    @Test fun justFinishedFeedCanBeLengthenedWithoutWaiting() {
        show(recent = true)
        val original = edits.pendingBreastEdit!!
        changeFirstMinutes("3")
        compose.onNodeWithText("Save changes").performClick()
        compose.waitUntil(5_000) { edits.pendingBreastEdit == null }
        assertEquals(original.startUtcMs - 60_000L, submitted!!.first().startUtcMs)
        assertEquals(original.finishUtcMs, submitted!!.last().endUtcMs)
    }

    @Test fun dateAndTimePickerCorrectsAnEarlierStart() {
        show()
        val earlier = LocalDateTime.now().minusDays(1).withHour(22).withMinute(15).withSecond(0).withNano(0)
        compose.onNodeWithText("Start:", substring = true).performClick()
        onView(isAssignableFrom(DatePicker::class.java)).perform(object : ViewAction {
            override fun getConstraints(): Matcher<View> = isAssignableFrom(DatePicker::class.java)
            override fun getDescription() = "Choose yesterday"
            override fun perform(controller: UiController, view: View) {
                (view as DatePicker).updateDate(earlier.year, earlier.monthValue - 1, earlier.dayOfMonth)
            }
        })
        onView(withId(android.R.id.button1)).perform(click())
        onView(isAssignableFrom(TimePicker::class.java)).perform(object : ViewAction {
            override fun getConstraints(): Matcher<View> = isAssignableFrom(TimePicker::class.java)
            override fun getDescription() = "Choose an earlier time"
            override fun perform(controller: UiController, view: View) {
                (view as TimePicker).hour = earlier.hour
                view.minute = earlier.minute
            }
        })
        onView(withId(android.R.id.button1)).perform(click())
        compose.onNodeWithText("Save changes").performClick()
        compose.waitUntil(5_000) { edits.pendingBreastEdit == null }
        val expectedStart = earlier.atZone(ZoneId.systemDefault()).toInstant().toEpochMilli()
        assertEquals(expectedStart, submitted!!.first().startUtcMs)
        assertEquals(expectedStart + 3 * 60_000L + 30_000L, submitted!!.last().endUtcMs)
    }

    @Test fun keepStartTimeStillAllowsAnOlderFeedToFinishLater() {
        show()
        compose.onNodeWithText("Keep start time").performClick()
        changeFirstMinutes("3")
        compose.onNodeWithText("Save changes").performClick()
        compose.waitUntil(5_000) { edits.pendingBreastEdit == null }
        assertEquals(start, submitted!!.first().startUtcMs)
        assertEquals(start + 4 * 60_000L + 30_000L, submitted!!.last().endUtcMs)
    }

    @Test fun futureEndKeepsEditorAndExplainsWhyThenAllowsCorrection() {
        show(recent = true)
        compose.onNodeWithText("Keep start time").performClick()
        changeFirstMinutes("3")
        compose.onNodeWithText("Save changes").performClick()
        compose.onNodeWithText("This feed would end in the future. Choose an earlier start or shorten the duration.")
            .assertIsDisplayed()
        assertEquals("3", edits.pendingBreastEdit!!.segments.first().second)
        assertEquals(0, calls)
        // Remove the second side and shorten the first, then retry the same entry.
        compose.onNodeWithText("Remove last segment").performScrollTo().performClick()
        changeFirstMinutes("1")
        compose.onNodeWithText("Save changes").performClick()
        compose.waitUntil(5_000) { edits.pendingBreastEdit == null }
        assertEquals(1, calls)
    }

    @Test fun failedWriteRetainsDraftAndAllowsRetry() {
        failSave = true
        show()
        changeFirstMinutes("3")
        compose.onNodeWithText("Save changes").performClick()
        // Wait for the dialog and keyboard relayout, not just the backing state.
        compose.waitUntil(5_000) { compose.onNodeWithText("Save failed").isDisplayed() }
        compose.onNodeWithText("Save failed").assertIsDisplayed()
        assertEquals("3", edits.pendingBreastEdit!!.segments.first().second)
        failSave = false
        compose.onNodeWithText("Save changes").performClick()
        compose.waitUntil(5_000) { edits.pendingBreastEdit == null }
        assertEquals(2, calls)
    }

    @Test fun savingKeepsDialogOpenAndPreventsDuplicateSubmissions() {
        val gate = CountDownLatch(1)
        holdSave = gate
        show()
        try {
            compose.onNodeWithText("Save changes").performClick()
            compose.waitUntil(5_000) { calls == 1 }
            assertNotNull(edits.pendingBreastEdit)
            compose.onNodeWithText("Save changes").assertIsNotEnabled()
            compose.onNodeWithText("Cancel").assertIsNotEnabled()
            compose.onAllNodesWithText("Minutes on selected side")[0].assertIsNotEnabled()
        } finally {
            gate.countDown()
        }
        compose.waitUntil(5_000) { edits.pendingBreastEdit == null }
        assertEquals(1, calls)
    }
}
