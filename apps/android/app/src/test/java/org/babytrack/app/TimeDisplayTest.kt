package org.babytrack.app

import android.app.Application
import java.time.Instant
import java.time.ZoneId
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], qualifiers = "en-rUS", application = Application::class)
class TimeDisplayTest {
    private val context = RuntimeEnvironment.getApplication()
    private val zone = ZoneId.of("UTC")
    private val now = Instant.parse("2026-09-29T14:00:00Z").toEpochMilli()

    private fun at(iso: String) = Instant.parse(iso).toEpochMilli()

    @Test
    fun compactDateTimeDropsRedundantDates() {
        assertEquals("1:35 PM", compactDateTime(context, at("2026-09-29T13:35:00Z"), now, zone))
        assertEquals(
            "Yesterday, 11:05 PM",
            compactDateTime(context, at("2026-09-28T23:05:00Z"), now, zone),
        )
        assertEquals("Sep 20, 8:00 AM", compactDateTime(context, at("2026-09-20T08:00:00Z"), now, zone))
        assertEquals(
            "Dec 31, 2025, 8:00 AM",
            compactDateTime(context, at("2025-12-31T08:00:00Z"), now, zone),
        )
    }

    @Test
    fun elapsedAndDurationLabels() {
        assertEquals("Just now", elapsedLabel(context, now - 30_000L, now))
        assertEquals("25 min ago", elapsedLabel(context, now - 25 * 60_000L, now))
        assertEquals("2 h ago", elapsedLabel(context, now - 120 * 60_000L, now))
        assertEquals("1 h 5 min ago", elapsedLabel(context, now - 65 * 60_000L, now))
        assertEquals("3 days ago", elapsedLabel(context, now - 3 * 24 * 60 * 60_000L, now))
        assertEquals("Just now", elapsedLabel(context, now + 60_000L, now))
        assertEquals("45 min", durationLabel(context, 45 * 60_000L))
        assertEquals("2 h", durationLabel(context, 120 * 60_000L))
        assertEquals("1 h 20 min", durationLabel(context, 80 * 60_000L))
        assertEquals("4:07", elapsedClock(247_000L))
        assertEquals("1:04:07", elapsedClock(3_847_000L))
    }
}
