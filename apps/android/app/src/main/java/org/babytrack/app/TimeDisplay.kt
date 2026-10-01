package org.babytrack.app

import android.content.Context
import java.text.DateFormat
import java.text.SimpleDateFormat
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.util.Date
import java.util.Locale
import java.util.TimeZone

// Display formatting only. Saved instants, offsets, and day totals come from the core.

private fun Context.locale(): Locale = resources.configuration.locales[0]

private fun localDay(utcMs: Long, zone: ZoneId): LocalDate =
    Instant.ofEpochMilli(utcMs).atZone(zone).toLocalDate()

internal fun clockTime(
    context: Context,
    utcMs: Long,
    zone: ZoneId = ZoneId.systemDefault(),
): String =
    android.text.format.DateFormat.getTimeFormat(context)
        .apply { timeZone = TimeZone.getTimeZone(zone) }
        .format(Date(utcMs))

private fun shortDate(context: Context, utcMs: Long, withYear: Boolean, zone: ZoneId): String {
    val skeleton = if (withYear) "yMMMd" else "MMMd"
    val pattern = android.text.format.DateFormat.getBestDateTimePattern(context.locale(), skeleton)
    return SimpleDateFormat(pattern, context.locale())
        .apply { timeZone = TimeZone.getTimeZone(zone) }
        .format(Date(utcMs))
}

/** Clock time today, "Yesterday" plus time, then a short date (year only when it differs). */
internal fun compactDateTime(
    context: Context,
    utcMs: Long,
    nowMs: Long,
    zone: ZoneId = ZoneId.systemDefault(),
): String {
    val day = localDay(utcMs, zone)
    val today = localDay(nowMs, zone)
    val time = clockTime(context, utcMs, zone)
    return when {
        day == today -> time
        day == today.minusDays(1) -> context.getString(R.string.yesterday_at, time)
        else ->
            context.getString(
                R.string.date_at,
                shortDate(context, utcMs, withYear = day.year != today.year, zone),
                time,
            )
    }
}

/** A day heading: Today, Yesterday, or a full localized date. */
internal fun dayHeading(context: Context, day: LocalDate, today: LocalDate, zone: ZoneId): String =
    when (day) {
        today -> context.getString(R.string.day_today)
        today.minusDays(1) -> context.getString(R.string.day_yesterday)
        else ->
            DateFormat.getDateInstance(DateFormat.FULL, context.locale())
                .format(Date.from(day.atStartOfDay(zone).toInstant()))
    }

/** Elapsed time since an event for glanceable state ("25 min ago"). */
internal fun elapsedLabel(context: Context, sinceMs: Long, nowMs: Long): String {
    val minutes = ((nowMs - sinceMs).coerceAtLeast(0L) / 60_000L).toInt()
    val resources = context.resources
    return when {
        minutes < 1 -> context.getString(R.string.elapsed_now)
        minutes < 60 -> resources.getQuantityString(R.plurals.elapsed_minutes, minutes, minutes)
        minutes < 24 * 60 ->
            if (minutes % 60 == 0)
                resources.getQuantityString(R.plurals.elapsed_hours, minutes / 60, minutes / 60)
            else context.getString(R.string.elapsed_hours_minutes, minutes / 60, minutes % 60)
        else -> {
            val days = minutes / (24 * 60)
            resources.getQuantityString(R.plurals.elapsed_days, days, days)
        }
    }
}

/** A compact duration such as "1 h 20 min" or "45 min". */
internal fun durationLabel(context: Context, durationMs: Long): String {
    val minutes = (durationMs.coerceAtLeast(0L) / 60_000L).toInt()
    return when {
        minutes < 60 -> context.getString(R.string.duration_minutes, minutes)
        minutes % 60 == 0 -> context.getString(R.string.duration_hours, minutes / 60)
        else -> context.getString(R.string.duration_hours_minutes, minutes / 60, minutes % 60)
    }
}

/** A running stopwatch: "4:07" under an hour, then "1:04:07". */
internal fun elapsedClock(elapsedMs: Long, locale: Locale = Locale.getDefault()): String {
    val seconds = elapsedMs.coerceAtLeast(0L) / 1000L
    val h = seconds / 3600
    val m = (seconds % 3600) / 60
    val s = seconds % 60
    return if (h > 0) String.format(locale, "%d:%02d:%02d", h, m, s)
    else String.format(locale, "%d:%02d", m, s)
}
