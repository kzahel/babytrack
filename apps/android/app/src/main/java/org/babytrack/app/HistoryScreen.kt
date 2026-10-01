package org.babytrack.app

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.CalendarMonth
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.TextStyle
import uniffi.babytrack_core_ffi.ActivityRow
import uniffi.babytrack_core_ffi.DaySummaryRow

internal data class HistoryUiState(
    /** ISO day for the Day view; null means today. */
    val selectedHistoryDay: String?,
    val timelineFilter: TimelineFilter,
    val entriesAreCurrent: Boolean,
    val entries: List<ActivityRow>,
    val expandedEntryKey: String?,
    val allDays: Boolean = false,
    /** The core's totals for the Day view's day, when loaded for it. */
    val daySummary: DaySummaryRow? = null,
    val nowMs: Long = 0L,
)

internal data class HistoryActions(
    val onChooseHistoryDay: () -> Unit = {},
    val onSelectDay: (String) -> Unit = { _ -> },
    val onShowAllDays: () -> Unit = {},
    val onShowDay: () -> Unit = {},
    val onTimelineFilterChange: (TimelineFilter) -> Unit = { _ -> },
    val onToggleEntryActions: (String) -> Unit = { _ -> },
    val onStopSleep: (ActivityRow) -> Unit = { _ -> },
    val onEditSleep: (ActivityRow) -> Unit = { _ -> },
    val onEditSleepPlace: (ActivityRow) -> Unit = { _ -> },
    val onAddActivityNote: (ActivityRow) -> Unit = { _ -> },
    val onEditEntryTime: (ActivityRow) -> Unit = { _ -> },
    val onMoveCompletedSession: (ActivityRow) -> Unit = { _ -> },
    val onEditBottle: (ActivityRow) -> Unit = { _ -> },
    val onEditBreast: (ActivityRow) -> Unit = { _ -> },
    val onEditDiaper: (ActivityRow) -> Unit = { _ -> },
    val onEditSolids: (ActivityRow) -> Unit = { _ -> },
    val onEditPump: (ActivityRow) -> Unit = { _ -> },
    val onEditMedication: (ActivityRow) -> Unit = { _ -> },
    val onEditGrowth: (ActivityRow) -> Unit = { _ -> },
    val onEditTemperature: (ActivityRow) -> Unit = { _ -> },
    val onDeleteEntry: (ActivityRow) -> Unit = { _ -> },
)

private val timelineFilters =
    listOf(
        TimelineFilter.ALL to R.string.timeline_all,
        TimelineFilter.FEEDS to R.string.timeline_feeds,
        TimelineFilter.SLEEP to R.string.timeline_sleep,
        TimelineFilter.DIAPERS to R.string.timeline_diapers,
        TimelineFilter.CARE to R.string.timeline_care,
        TimelineFilter.NOTES to R.string.timeline_notes,
    )

@Composable
internal fun ColumnScope.HistoryScreen(state: HistoryUiState, actions: HistoryActions) {
    val context = LocalContext.current
    with(state) {
        val zone = ZoneId.systemDefault()
        val today = Instant.ofEpochMilli(nowMs).atZone(zone).toLocalDate()
        val day = selectedHistoryDay?.let(LocalDate::parse) ?: today
        SegmentedChoice(
            listOf(
                false to stringResource(R.string.history_day),
                true to stringResource(R.string.history_all_days),
            ),
            allDays,
            { all -> if (all) actions.onShowAllDays() else actions.onShowDay() },
        )
        if (!allDays) WeekStrip(day, today, actions)
        ChipRow(
            timelineFilters.map { (filter, label) -> filter to stringResource(label) },
            timelineFilter,
            actions.onTimelineFilterChange,
        )
        val currentEntries = if (entriesAreCurrent) entries else emptyList()
        daySummary
            ?.takeIf {
                !allDays && (it.feedCount > 0uL || it.diaperCount > 0uL || it.sleepMs > 0uL)
            }
            ?.let { DaySummaryCard(it) }
        val dayStart = day.atStartOfDay(zone).toInstant().toEpochMilli()
        val dayEnd = day.plusDays(1).atStartOfDay(zone).toInstant().toEpochMilli()
        val visibleEntries =
            currentEntries
                .filter { entry ->
                    // A sleep or feed that crosses midnight is listed on each day it touches.
                    val end = maxOf(entry.endUtcMs ?: nowMs, entry.startUtcMs)
                    timelineFilter.includes(entry.kind) &&
                        (allDays ||
                            (entry.startUtcMs < dayEnd &&
                                (end > dayStart || entry.startUtcMs >= dayStart)))
                }
                .sortedByDescending { it.startUtcMs }
        if (visibleEntries.isEmpty())
            Text(
                stringResource(
                    if (currentEntries.isEmpty()) R.string.no_entries
                    else R.string.no_matching_entries
                ),
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        visibleEntries
            .groupBy {
                if (allDays) Instant.ofEpochMilli(it.startUtcMs).atZone(zone).toLocalDate() else day
            }
            .forEach { (entryDay, dayEntries) ->
                if (allDays) SectionHeader(dayHeading(context, entryDay, today, zone))
                SectionCard { dayEntries.forEach { entry -> HistoryEntry(state, entry, actions) } }
            }
    }
}

@Composable
private fun WeekStrip(day: LocalDate, today: LocalDate, actions: HistoryActions) {
    val context = LocalContext.current
    val locale = context.resources.configuration.locales[0]
    val end = minOf(today, day.plusDays(3))
    val days = (6 downTo 0).map { end.minusDays(it.toLong()) }
    Row(
        Modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        days.forEach { candidate ->
            val selected = candidate == day
            Surface(
                shape = MaterialTheme.shapes.small,
                color =
                    if (selected) MaterialTheme.colorScheme.primary
                    else MaterialTheme.colorScheme.surfaceContainer,
                contentColor =
                    if (selected) MaterialTheme.colorScheme.onPrimary
                    else MaterialTheme.colorScheme.onSurface,
                modifier =
                    Modifier.weight(1f)
                        .heightIn(min = 56.dp)
                        .clip(MaterialTheme.shapes.small)
                        .semantics { this.selected = selected }
                        .clickable(role = Role.Tab) { actions.onSelectDay(candidate.toString()) },
            ) {
                Column(
                    Modifier.padding(vertical = 6.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center,
                ) {
                    Text(
                        candidate.dayOfWeek.getDisplayName(TextStyle.NARROW, locale),
                        style = MaterialTheme.typography.labelMedium,
                        textAlign = TextAlign.Center,
                    )
                    Text(
                        candidate.dayOfMonth.toString(),
                        style = MaterialTheme.typography.titleMedium,
                        textAlign = TextAlign.Center,
                    )
                }
            }
        }
        IconButton(onClick = actions.onChooseHistoryDay) {
            Icon(
                Icons.Outlined.CalendarMonth,
                contentDescription = stringResource(R.string.choose_history_day),
            )
        }
    }
    // The strip already marks today; name other days so the selection is explicit.
    if (day != today)
        Text(
            dayHeading(context, day, today, ZoneId.systemDefault()),
            style = MaterialTheme.typography.titleMedium,
        )
}

@Composable
private fun DaySummaryCard(summary: DaySummaryRow) {
    val context = LocalContext.current
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        StatCell(
            "feed.bottle",
            summary.feedCount.toString(),
            pluralStringResource(R.plurals.history_feeds_label, summary.feedCount.toInt()),
            if (summary.bottleMl > 0uL)
                stringResource(R.string.history_bottle_total, summary.bottleMl.toLong())
            else null,
            Modifier.weight(1f),
        )
        StatCell(
            "sleep",
            durationLabel(context, summary.sleepMs.toLong()),
            stringResource(R.string.history_sleep_label),
            null,
            Modifier.weight(1f),
        )
        StatCell(
            "diaper",
            summary.diaperCount.toString(),
            pluralStringResource(R.plurals.history_diapers_label, summary.diaperCount.toInt()),
            if (summary.diaperCount > 0uL)
                stringResource(
                    R.string.history_diaper_split,
                    summary.wetDiaperCount.toLong(),
                    summary.dirtyDiaperCount.toLong(),
                )
            else null,
            Modifier.weight(1f),
        )
    }
}

@Composable
private fun StatCell(
    kind: String,
    value: String,
    label: String,
    detail: String?,
    modifier: Modifier,
) {
    val colors = categoryColors(kind)
    Surface(shape = MaterialTheme.shapes.medium, color = colors.container, modifier = modifier) {
        Column(
            Modifier.padding(horizontal = 10.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Icon(
                    activityIcon(kind),
                    contentDescription = null,
                    tint = colors.accent,
                    modifier = Modifier.size(18.dp),
                )
                Text(label, style = MaterialTheme.typography.labelLarge)
            }
            Text(value, style = MaterialTheme.typography.titleMedium)
            if (detail != null)
                Text(
                    detail,
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun HistoryEntry(state: HistoryUiState, entry: ActivityRow, actions: HistoryActions) {
    val context = LocalContext.current
    val entryKey = entry.id.key()
    val expanded = state.expandedEntryKey == entryKey
    val details =
        listOfNotNull(
            entry.note
                ?.takeIf { entry.kind != "note" }
                ?.let { stringResource(R.string.activity_note, it) },
            entry.sleepPlace
                ?.takeIf { entry.kind == "sleep" }
                ?.let { stringResource(R.string.sleep_place_entry, stringResource(sleepPlaceLabel(it))) },
        )
    Column {
        EntryRow(
            kind = entry.kind,
            title = entrySummary(context, entry),
            details = details,
            time = clockTime(context, entry.startUtcMs),
            modifier = Modifier.testTag("entry-row"),
            onClickLabel =
                stringResource(
                    if (expanded) R.string.hide_entry_actions else R.string.show_entry_actions
                ),
            onClick = { actions.onToggleEntryActions(entryKey) },
        )
        if (expanded)
            FlowRow(
                Modifier.fillMaxWidth().padding(start = 56.dp, bottom = 8.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                EntryActionButtons(entry, actions)
            }
    }
}

@Composable
private fun EntryActionButtons(entry: ActivityRow, actions: HistoryActions) {
    if (entry.kind == "sleep" && entry.endUtcMs == null)
        Button(onClick = { actions.onStopSleep(entry) }) { Text(stringResource(R.string.stop_sleep)) }
    if (entry.kind == "sleep" && entry.endUtcMs != null)
        EntryAction(R.string.edit_sleep) { actions.onEditSleep(entry) }
    if (entry.kind == "sleep")
        EntryAction(R.string.edit_sleep_place) { actions.onEditSleepPlace(entry) }
    if (entry.kind in noteEditableKinds)
        EntryAction(if (entry.note == null) R.string.add_activity_note else R.string.edit_note) {
            actions.onAddActivityNote(entry)
        }
    if (entry.kind in instantTimeEditableKinds)
        EntryAction(R.string.edit_entry_time) { actions.onEditEntryTime(entry) }
    if (
        (entry.kind == "sleep" || entry.kind == "pump") &&
            entry.endUtcMs != null &&
            entry.endUtcMs!! > entry.startUtcMs
    )
        EntryAction(R.string.move_completed_session) { actions.onMoveCompletedSession(entry) }
    if (entry.kind == "feed.bottle" && entry.bottleMl != null)
        EntryAction(R.string.edit_bottle) { actions.onEditBottle(entry) }
    if (
        entry.kind == "feed.breast" &&
            entry.breastSegments?.all { (it.endUtcMs - it.startUtcMs) % 60_000L == 0L } == true
    )
        EntryAction(R.string.edit_breast) { actions.onEditBreast(entry) }
    if (entry.kind == "diaper" && entry.diaperKind != null)
        EntryAction(R.string.edit_diaper) { actions.onEditDiaper(entry) }
    if (entry.kind == "feed.solids" && entry.solidsFoods != null)
        EntryAction(R.string.edit_solids) { actions.onEditSolids(entry) }
    if (entry.kind == "pump") EntryAction(R.string.edit_pump) { actions.onEditPump(entry) }
    if (entry.kind == "medication" && entry.medicationName != null)
        EntryAction(R.string.edit_medication) { actions.onEditMedication(entry) }
    if (entry.kind == "growth") EntryAction(R.string.edit_growth) { actions.onEditGrowth(entry) }
    if (entry.kind == "temperature" && entry.temperatureC != null)
        EntryAction(R.string.edit_temperature) { actions.onEditTemperature(entry) }
    EntryAction(R.string.delete_entry) { actions.onDeleteEntry(entry) }
}

@Composable
private fun EntryAction(label: Int, onClick: () -> Unit) {
    OutlinedButton(onClick = onClick) { Text(stringResource(label)) }
}
