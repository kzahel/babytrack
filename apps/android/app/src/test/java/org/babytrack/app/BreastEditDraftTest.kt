package org.babytrack.app

import java.time.LocalDateTime
import java.time.ZoneId
import java.util.TimeZone
import org.junit.Assert.*
import org.junit.Test
import uniffi.babytrack_core_ffi.FamilyRef

class BreastEditDraftTest {
    private val minute = 60_000L
    private fun draft() = PendingBreastEdit(
        FamilyRef(ByteArray(16), ByteArray(16)), ByteArray(16), ByteArray(16), false,
        1_000_000L, 0, listOf(1u.toUByte() to "2", 2u.toUByte() to "1"), listOf(0L, 30_000L),
        finishUtcMs = 1_000_000L + 3 * minute + 30_000L,
    )

    @Test fun lengtheningAndShorteningKeepTheFinishAndPause() {
        val original = draft()
        val longer = original.copy(segments = listOf(1u.toUByte() to "3", 2u.toUByte() to "1"))
        assertEquals(original.startUtcMs - minute, longer.plannedStartMs())
        assertEquals(original.finishUtcMs, longer.plannedFinishMs())
        val shorter = original.copy(segments = listOf(1u.toUByte() to "1", 2u.toUByte() to "1"))
        assertEquals(original.startUtcMs + minute, shorter.plannedStartMs())
        assertEquals(original.finishUtcMs, shorter.plannedFinishMs())
        assertEquals(original.gapsMs, longer.gapsMs)
    }

    @Test fun emptyInputDoesNotLoseTheFinishAnchor() {
        val empty = draft().copy(segments = listOf(1u.toUByte() to "", 2u.toUByte() to "1"))
        assertNull(empty.plannedStartMs())
        val completed = empty.copy(segments = listOf(1u.toUByte() to "3", 2u.toUByte() to "1"))
        assertEquals(draft().startUtcMs - minute, completed.plannedStartMs())
        assertEquals(draft().finishUtcMs, completed.plannedFinishMs())
    }

    @Test fun changingAnchorKeepsTheVisibleWindowThenMovesTheOtherEndpoint() {
        val longer = draft().copy(segments = listOf(1u.toUByte() to "3", 2u.toUByte() to "1"))
        val fixedStart = longer.keepingFinish(false)
        assertEquals(longer.plannedStartMs(), fixedStart.plannedStartMs())
        assertEquals(longer.plannedFinishMs(), fixedStart.plannedFinishMs())
        val changed = fixedStart.copy(segments = listOf(1u.toUByte() to "4", 2u.toUByte() to "1"))
        assertEquals(fixedStart.plannedStartMs(), changed.plannedStartMs())
        assertEquals(fixedStart.plannedFinishMs()!! + minute, changed.plannedFinishMs())
    }

    @Test fun earlierStartKeepsDurationsAcrossMidnightAndUsesItsZoneOffset() {
        val previous = TimeZone.getDefault()
        try {
            TimeZone.setDefault(TimeZone.getTimeZone("Europe/Berlin"))
            val start = LocalDateTime.of(2026, 10, 24, 23, 59).atZone(ZoneId.systemDefault()).toInstant().toEpochMilli()
            for (keepFinish in listOf(true, false)) {
                val changed = draft().keepingFinish(keepFinish).startingAt(start)
                assertEquals(start, changed.plannedStartMs())
                assertEquals(start + draft().spanMs()!!, changed.plannedFinishMs())
                assertEquals(120.toShort(), changed.startOffsetMinutes)
                assertEquals(draft().segments, changed.segments)
            }
            val afterDst = LocalDateTime.of(2026, 10, 25, 3, 0).atZone(ZoneId.systemDefault()).toInstant().toEpochMilli()
            assertEquals(60.toShort(), draft().startingAt(afterDst).startOffsetMinutes)
        } finally {
            TimeZone.setDefault(previous)
        }
    }
}
