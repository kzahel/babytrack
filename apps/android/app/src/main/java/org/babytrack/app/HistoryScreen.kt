package org.babytrack.app

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import java.text.DateFormat
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.util.Date
import uniffi.babytrack_core_ffi.ActivityRow

internal data class HistoryUiState(
    val selectedHistoryDay: String?,
    val timelineFilter: TimelineFilter,
    val entriesAreCurrent: Boolean,
    val entries: List<ActivityRow>,
    val expandedEntryKey: String?,
)

internal data class HistoryActions(
    val onChooseHistoryDay: () -> Unit = {},
    val onShowAllDays: () -> Unit = {},
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

@Composable
internal fun ColumnScope.HistoryScreen(state: HistoryUiState, actions: HistoryActions) {
    val context = LocalContext.current
    with(state) {
        Text(stringResource(R.string.timeline), style = MaterialTheme.typography.titleLarge)
        OutlinedButton(onClick = actions.onChooseHistoryDay) {
            Text(
                selectedHistoryDay?.let { iso ->
                    val day = LocalDate.parse(iso)
                    DateFormat.getDateInstance(DateFormat.MEDIUM)
                        .format(Date.from(day.atStartOfDay(ZoneId.systemDefault()).toInstant()))
                } ?: stringResource(R.string.choose_history_day)
            )
        }
        if (selectedHistoryDay != null)
            TextButton(onClick = actions.onShowAllDays) {
                Text(stringResource(R.string.show_all_days))
            }
        listOf(
                TimelineFilter.ALL to R.string.timeline_all,
                TimelineFilter.FEEDS to R.string.timeline_feeds,
                TimelineFilter.SLEEP to R.string.timeline_sleep,
                TimelineFilter.DIAPERS to R.string.timeline_diapers,
                TimelineFilter.CARE to R.string.timeline_care,
                TimelineFilter.NOTES to R.string.timeline_notes,
            )
            .chunked(2)
            .forEach { options ->
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    options.forEach { (filter, label) ->
                        FilterChip(
                            selected = timelineFilter == filter,
                            onClick = { actions.onTimelineFilterChange(filter) },
                            label = { Text(stringResource(label)) },
                        )
                    }
                }
            }
        val currentEntries = if (entriesAreCurrent) entries else emptyList()
        val visibleEntries =
            currentEntries.filter { entry ->
                timelineFilter.includes(entry.kind) &&
                    (selectedHistoryDay == null ||
                        Instant.ofEpochMilli(entry.startUtcMs)
                            .atZone(ZoneId.systemDefault())
                            .toLocalDate()
                            .toString() == selectedHistoryDay)
            }
        if (visibleEntries.isEmpty())
            Text(
                stringResource(
                    if (currentEntries.isEmpty()) R.string.no_entries
                    else R.string.no_matching_entries
                )
            )
        var previousDay: LocalDate? = null
        visibleEntries.forEach { entry ->
            val day =
                Instant.ofEpochMilli(entry.startUtcMs).atZone(ZoneId.systemDefault()).toLocalDate()
            if (day != previousDay) {
                Text(
                    DateFormat.getDateInstance(DateFormat.FULL).format(Date(entry.startUtcMs)),
                    style = MaterialTheme.typography.titleMedium,
                )
                previousDay = day
            }
            val label = entrySummary(context, entry)
            Card(Modifier.fillMaxWidth()) {
                Column(Modifier.padding(12.dp)) {
                    Text(label, fontWeight = FontWeight.SemiBold)
                    Text(
                        DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT)
                            .format(Date(entry.startUtcMs))
                    )
                    if (entry.kind != "note" && entry.note != null) {
                        Text(stringResource(R.string.activity_note, entry.note!!))
                    }
                    if (entry.kind == "sleep" && entry.sleepPlace != null) {
                        val placeLabel =
                            when (entry.sleepPlace!!.toInt()) {
                                1 -> R.string.sleep_place_crib
                                2 -> R.string.sleep_place_pram
                                3 -> R.string.sleep_place_contact
                                4 -> R.string.sleep_place_car
                                else -> R.string.sleep_place_other
                            }
                        Text(stringResource(R.string.sleep_place_entry, stringResource(placeLabel)))
                    }
                    val entryKey = entry.id.key()
                    TextButton(onClick = { actions.onToggleEntryActions(entryKey) }) {
                        Text(
                            stringResource(
                                if (expandedEntryKey == entryKey) R.string.hide_entry_actions
                                else R.string.show_entry_actions
                            )
                        )
                    }
                    if (expandedEntryKey == entryKey) {
                        if (entry.kind == "sleep" && entry.endUtcMs == null) {
                            Button(onClick = { actions.onStopSleep(entry) }) {
                                Text(stringResource(R.string.stop_sleep))
                            }
                        }
                        if (entry.kind == "sleep" && entry.endUtcMs != null) {
                            OutlinedButton(onClick = { actions.onEditSleep(entry) }) {
                                Text(stringResource(R.string.edit_sleep))
                            }
                        }
                        if (entry.kind == "sleep") {
                            OutlinedButton(onClick = { actions.onEditSleepPlace(entry) }) {
                                Text(stringResource(R.string.edit_sleep_place))
                            }
                        }
                        if (entry.kind in noteEditableKinds) {
                            OutlinedButton(onClick = { actions.onAddActivityNote(entry) }) {
                                Text(
                                    stringResource(
                                        if (entry.note == null) R.string.add_activity_note
                                        else R.string.edit_note
                                    )
                                )
                            }
                        }
                        if (entry.kind in instantTimeEditableKinds) {
                            OutlinedButton(onClick = { actions.onEditEntryTime(entry) }) {
                                Text(stringResource(R.string.edit_entry_time))
                            }
                        }
                        if (
                            (entry.kind == "sleep" || entry.kind == "pump") &&
                                entry.endUtcMs != null &&
                                entry.endUtcMs!! > entry.startUtcMs
                        ) {
                            OutlinedButton(onClick = { actions.onMoveCompletedSession(entry) }) {
                                Text(stringResource(R.string.move_completed_session))
                            }
                        }
                        if (entry.kind == "feed.bottle" && entry.bottleMl != null) {
                            OutlinedButton(onClick = { actions.onEditBottle(entry) }) {
                                Text(stringResource(R.string.edit_bottle))
                            }
                        }
                        if (
                            entry.kind == "feed.breast" &&
                                entry.breastSegments?.all {
                                    (it.endUtcMs - it.startUtcMs) % 60_000L == 0L
                                } == true
                        ) {
                            OutlinedButton(onClick = { actions.onEditBreast(entry) }) {
                                Text(stringResource(R.string.edit_breast))
                            }
                        }
                        if (entry.kind == "diaper" && entry.diaperKind != null) {
                            OutlinedButton(onClick = { actions.onEditDiaper(entry) }) {
                                Text(stringResource(R.string.edit_diaper))
                            }
                        }
                        if (entry.kind == "feed.solids" && entry.solidsFoods != null) {
                            OutlinedButton(onClick = { actions.onEditSolids(entry) }) {
                                Text(stringResource(R.string.edit_solids))
                            }
                        }
                        if (entry.kind == "pump") {
                            OutlinedButton(onClick = { actions.onEditPump(entry) }) {
                                Text(stringResource(R.string.edit_pump))
                            }
                        }
                        if (entry.kind == "medication" && entry.medicationName != null) {
                            OutlinedButton(onClick = { actions.onEditMedication(entry) }) {
                                Text(stringResource(R.string.edit_medication))
                            }
                        }
                        if (entry.kind == "growth") {
                            OutlinedButton(onClick = { actions.onEditGrowth(entry) }) {
                                Text(stringResource(R.string.edit_growth))
                            }
                        }
                        if (entry.kind == "temperature" && entry.temperatureC != null) {
                            OutlinedButton(onClick = { actions.onEditTemperature(entry) }) {
                                Text(stringResource(R.string.edit_temperature))
                            }
                        }
                        OutlinedButton(onClick = { actions.onDeleteEntry(entry) }) {
                            Text(stringResource(R.string.delete_entry))
                        }
                    }
                }
            }
        }
    }
}
