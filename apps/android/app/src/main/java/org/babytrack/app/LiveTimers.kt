package org.babytrack.app

import android.content.SharedPreferences
import org.json.JSONArray
import org.json.JSONObject

/**
 * Unsaved, device-local timer drafts (see the event model's timer section).
 * They become ordinary Rust events only when the caregiver saves.
 */
internal data class TimedSegment(val side: UByte, val startMs: Long, val endMs: Long?)

internal data class PlannedSegment(val side: UByte, val startMs: Long, val endMs: Long)

internal const val maxBreastSegments = 8
internal const val maxBreastMinutes = 240L

internal fun List<TimedSegment>.running(): TimedSegment? = lastOrNull()?.takeIf { it.endMs == null }

/** Tap a side: start it, switch to it, or pause it if it is already running. */
internal fun tapNursingSide(segments: List<TimedSegment>, side: UByte, nowMs: Long): List<TimedSegment> {
    val running = segments.running()
    val closed =
        if (running != null) segments.dropLast(1) + running.copy(endMs = maxOf(nowMs, running.startMs))
        else segments
    return when {
        running?.side == side -> closed
        closed.size >= maxBreastSegments -> closed
        else -> closed + TimedSegment(side, nowMs, null)
    }
}

internal fun sideElapsedMs(segments: List<TimedSegment>, side: UByte, nowMs: Long): Long =
    segments.filter { it.side == side }.sumOf { (it.endMs ?: nowMs) - it.startMs }.coerceAtLeast(0L)

internal fun totalElapsedMs(segments: List<TimedSegment>, nowMs: Long): Long =
    segments.sumOf { (it.endMs ?: nowMs) - it.startMs }.coerceAtLeast(0L)

/**
 * Whole-minute segments for saving, so the entry stays correctable with the
 * minute-based correction form. Each timed side keeps at least one minute,
 * order and pauses are kept, and the result never ends after [nowMs].
 * Returns null when nothing was timed or the core's limits would be exceeded.
 */
internal fun plannedBreastSegments(segments: List<TimedSegment>, nowMs: Long): List<PlannedSegment>? {
    val timed =
        segments
            .map { it.copy(endMs = it.endMs ?: nowMs) }
            .filter { it.endMs!! - it.startMs >= 1_000L }
    if (timed.isEmpty() || timed.size > maxBreastSegments) return null
    var cursor = timed.first().startMs
    val planned =
        timed.map { segment ->
            val start = maxOf(segment.startMs, cursor)
            val minutes = maxOf(1L, Math.round((segment.endMs!! - segment.startMs) / 60_000.0))
            PlannedSegment(segment.side, start, start + minutes * 60_000L).also { cursor = it.endMs }
        }
    if (planned.sumOf { (it.endMs - it.startMs) / 60_000L } > maxBreastMinutes) return null
    val overshoot = planned.last().endMs - nowMs
    return if (overshoot > 0)
        planned.map { it.copy(startMs = it.startMs - overshoot, endMs = it.endMs - overshoot) }
    else planned
}

/** Rounded minutes for a pumping stopwatch, at least one. */
internal fun stopwatchMinutes(startMs: Long, endMs: Long): Long =
    maxOf(1L, Math.round((endMs - startMs).coerceAtLeast(0L) / 60_000.0))

/** Private app preferences keyed by Family and child; never canonical records. */
internal class LiveTimerStore(private val prefs: SharedPreferences) {
    fun nursing(target: String): List<TimedSegment> =
        runCatching {
                val array = JSONArray(prefs.getString("nursing:$target", "[]"))
                (0 until array.length()).map { index ->
                    val item = array.getJSONObject(index)
                    TimedSegment(
                        item.getInt("side").toUByte(),
                        item.getLong("start"),
                        if (item.has("end")) item.getLong("end") else null,
                    )
                }
            }
            .getOrDefault(emptyList())

    fun saveNursing(target: String, segments: List<TimedSegment>) {
        val editor = prefs.edit()
        if (segments.isEmpty()) editor.remove("nursing:$target")
        else
            editor.putString(
                "nursing:$target",
                JSONArray(
                        segments.map { segment ->
                            JSONObject()
                                .put("side", segment.side.toInt())
                                .put("start", segment.startMs)
                                .apply { segment.endMs?.let { put("end", it) } }
                        }
                    )
                    .toString(),
            )
        editor.apply()
    }

    fun pumpStart(target: String): Long? =
        if (prefs.contains("pump:$target")) prefs.getLong("pump:$target", 0L) else null

    fun savePumpStart(target: String, startMs: Long?) {
        val editor = prefs.edit()
        if (startMs == null) editor.remove("pump:$target") else editor.putLong("pump:$target", startMs)
        editor.apply()
    }
}

internal fun timerTarget(familyKey: String, childKey: String): String = "$familyKey:$childKey"
