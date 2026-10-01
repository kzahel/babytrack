package org.babytrack.app

import android.app.Application
import android.content.Context
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], application = Application::class)
class LiveTimersTest {
    private val left = 1u.toUByte()
    private val right = 2u.toUByte()
    private val minute = 60_000L

    @Test
    fun tappingSidesStartsSwitchesAndPauses() {
        var segments = tapNursingSide(emptyList(), left, 0L)
        assertEquals(listOf(TimedSegment(left, 0L, null)), segments)
        segments = tapNursingSide(segments, right, 5 * minute)
        assertEquals(TimedSegment(left, 0L, 5 * minute), segments[0])
        assertEquals(TimedSegment(right, 5 * minute, null), segments[1])
        segments = tapNursingSide(segments, right, 8 * minute)
        assertNull(segments.running())
        assertEquals(5 * minute, sideElapsedMs(segments, left, 20 * minute))
        assertEquals(3 * minute, sideElapsedMs(segments, right, 20 * minute))
        assertEquals(8 * minute, totalElapsedMs(segments, 20 * minute))
    }

    @Test
    fun segmentLimitStopsNewSidesButStillPauses() {
        var segments = emptyList<TimedSegment>()
        repeat(maxBreastSegments) { index ->
            segments = tapNursingSide(segments, if (index % 2 == 0) left else right, index * minute)
        }
        segments = tapNursingSide(segments, left, 20 * minute)
        assertEquals(maxBreastSegments, segments.size)
        assertNull(segments.running())
    }

    @Test
    fun plannedSegmentsAreWholeMinutesInOrderAndNeverInTheFuture() {
        val start = 1_000_000L
        val segments =
            listOf(
                TimedSegment(left, start, start + 4 * minute + 40_000L),
                // A 20-second pause, then a short right side.
                TimedSegment(right, start + 5 * minute, start + 5 * minute + 20_000L),
                TimedSegment(left, start + 6 * minute, null),
            )
        val now = start + 9 * minute + 10_000L
        val planned = plannedBreastSegments(segments, now)!!
        assertEquals(listOf(left, right, left), planned.map { it.side })
        planned.forEach { assertEquals(0L, (it.endMs - it.startMs) % minute) }
        assertEquals(listOf(5L, 1L, 3L), planned.map { (it.endMs - it.startMs) / minute })
        planned.zipWithNext().forEach { (a, b) -> assertTrue(b.startMs >= a.endMs) }
        assertTrue(planned.last().endMs <= now)
    }

    @Test
    fun nothingTimedOrTooLongCannotBeSaved() {
        assertNull(plannedBreastSegments(emptyList(), 0L))
        assertNull(plannedBreastSegments(listOf(TimedSegment(left, 0L, 500L)), 1_000L))
        assertNull(plannedBreastSegments(listOf(TimedSegment(left, 0L, 241 * minute)), 241 * minute))
        assertEquals(1L, stopwatchMinutes(0L, 10_000L))
        assertEquals(12L, stopwatchMinutes(0L, 12 * minute + 20_000L))
    }

    @Test
    fun storeRoundTripsPerTarget() {
        val prefs =
            RuntimeEnvironment.getApplication()
                .getSharedPreferences("live_timers_test", Context.MODE_PRIVATE)
        val store = LiveTimerStore(prefs)
        val segments = listOf(TimedSegment(left, 1L, 2L), TimedSegment(right, 3L, null))
        store.saveNursing("a:b", segments)
        store.savePumpStart("a:b", 42L)
        assertEquals(segments, store.nursing("a:b"))
        assertEquals(emptyList<TimedSegment>(), store.nursing("a:c"))
        assertEquals(42L, store.pumpStart("a:b"))
        store.saveNursing("a:b", emptyList())
        store.savePumpStart("a:b", null)
        assertEquals(emptyList<TimedSegment>(), store.nursing("a:b"))
        assertNull(store.pumpStart("a:b"))
    }
}
