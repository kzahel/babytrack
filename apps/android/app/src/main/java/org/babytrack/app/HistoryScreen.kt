package org.babytrack.app

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import uniffi.babytrack_core_ffi.*
import java.text.DateFormat
import java.time.*
import java.util.Date

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
            Text(selectedHistoryDay?.let { iso ->
                val day = LocalDate.parse(iso)
                DateFormat.getDateInstance(DateFormat.MEDIUM).format(
                    Date.from(day.atStartOfDay(ZoneId.systemDefault()).toInstant()))
            } ?: stringResource(R.string.choose_history_day))
        }
        if (selectedHistoryDay != null) TextButton(onClick = actions.onShowAllDays) { Text(stringResource(R.string.show_all_days)) }
        listOf(
            TimelineFilter.ALL to R.string.timeline_all,
            TimelineFilter.FEEDS to R.string.timeline_feeds,
            TimelineFilter.SLEEP to R.string.timeline_sleep,
            TimelineFilter.DIAPERS to R.string.timeline_diapers,
            TimelineFilter.CARE to R.string.timeline_care,
            TimelineFilter.NOTES to R.string.timeline_notes,
        ).chunked(2).forEach { options ->
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                options.forEach { (filter, label) ->
                    FilterChip(selected = timelineFilter == filter,
                        onClick = { actions.onTimelineFilterChange(filter) },
                        label = { Text(stringResource(label)) })
                }
            }
        }
        val currentEntries = if (entriesAreCurrent) entries else emptyList()
        val visibleEntries = currentEntries.filter { entry ->
            timelineFilter.includes(entry.kind) &&
                (selectedHistoryDay == null || Instant.ofEpochMilli(entry.startUtcMs)
                    .atZone(ZoneId.systemDefault()).toLocalDate().toString() == selectedHistoryDay)
        }
        if (visibleEntries.isEmpty()) Text(stringResource(
            if (currentEntries.isEmpty()) R.string.no_entries else R.string.no_matching_entries))
        var previousDay: LocalDate? = null
        visibleEntries.forEach { entry ->
            val day = Instant.ofEpochMilli(entry.startUtcMs)
                .atZone(ZoneId.systemDefault()).toLocalDate()
            if (day != previousDay) {
                Text(DateFormat.getDateInstance(DateFormat.FULL).format(Date(entry.startUtcMs)),
                    style = MaterialTheme.typography.titleMedium)
                previousDay = day
            }
            val label = when {
                entry.bottleMl != null -> stringResource(R.string.bottle_with_entered,
                    localizedEntered(context, entry.bottleEntered ?: entry.bottleMl.toString()),
                    stringResource(when (entry.bottleUnit) {
                        2u.toUByte() -> R.string.unit_us_fl_oz
                        3u.toUByte() -> R.string.unit_uk_fl_oz
                        else -> R.string.unit_ml
                    }),
                    stringResource(when (entry.bottleContent) {
                        1u.toUByte() -> R.string.bottle_breast_milk
                        2u.toUByte() -> R.string.bottle_formula
                        3u.toUByte() -> R.string.bottle_mixed
                        else -> R.string.bottle_other
                    }))
                entry.kind == "feed.breast" && entry.breastSide != null && entry.endUtcMs != null ->
                    stringResource(R.string.breast_entry,
                        stringResource(if (entry.breastSide == 1u.toUByte()) R.string.breast_left else R.string.breast_right),
                        (entry.endUtcMs!! - entry.startUtcMs) / 60_000L)
                entry.kind == "feed.breast" && entry.breastSegments != null ->
                    stringResource(R.string.breast_multi_entry,
                        entry.breastSegments!!.joinToString(" → ") { segment ->
                            context.getString(R.string.breast_segment_summary,
                                context.getString(if (segment.side == 1u.toUByte()) R.string.breast_left else R.string.breast_right),
                                (segment.endUtcMs - segment.startUtcMs) / 60_000L)
                        })
                entry.kind == "pump" && entry.pumpTotalMl != null && entry.endUtcMs != null ->
                    stringResource(R.string.pump_total_entry, entry.pumpTotalMl!!,
                        (entry.endUtcMs!! - entry.startUtcMs) / 60_000L)
                entry.kind == "pump" && entry.endUtcMs != null &&
                    (entry.pumpLeftMl != null || entry.pumpRightMl != null) ->
                    stringResource(R.string.pump_sides_entry, entry.pumpLeftMl ?: 0L,
                        entry.pumpRightMl ?: 0L, (entry.endUtcMs!! - entry.startUtcMs) / 60_000L)
                entry.kind == "feed.solids" && entry.solidsFoods != null -> {
                    val foods = entry.solidsFoods!!.joinToString(", ")
                    if (entry.solidsAmount.isNullOrBlank()) stringResource(R.string.solids_entry, foods)
                    else stringResource(R.string.solids_entry_amount, foods, entry.solidsAmount!!)
                }
                entry.kind == "sleep" && entry.endUtcMs != null ->
                    stringResource(R.string.sleep_duration, (entry.endUtcMs!! - entry.startUtcMs) / 60_000)
                entry.kind == "sleep" -> stringResource(R.string.sleep_running)
                entry.kind == "note" && entry.note != null -> stringResource(R.string.note_entry, entry.note!!)
                entry.kind == "growth" -> {
                    val parts = listOfNotNull(
                        growthDisplay(context, entry.growthWeightG, entry.growthWeightEntered,
                            entry.growthWeightUnit, 10u.toUByte()),
                        growthDisplay(context, entry.growthLengthMm, entry.growthLengthEntered,
                            entry.growthLengthUnit, 20u.toUByte()),
                        growthDisplay(context, entry.growthHeadMm, entry.growthHeadEntered,
                            entry.growthHeadUnit, 20u.toUByte())?.let {
                            context.getString(R.string.growth_head_part, it)
                        },
                    )
                    if (parts.isEmpty()) entry.kind
                    else stringResource(R.string.growth_summary, parts.joinToString(" · "))
                }
                entry.kind == "temperature" && entry.temperatureC != null ->
                    stringResource(R.string.temperature_entry,
                        localizedEntered(context, entry.temperatureEntered ?: entry.temperatureC!!),
                        stringResource(if (entry.temperatureUnit == 31u.toUByte())
                            R.string.unit_fahrenheit else R.string.unit_celsius))
                entry.kind == "medication" && entry.medicationName != null &&
                    entry.medicationDoseAmount != null && entry.medicationDoseUnit != null ->
                    stringResource(
                        R.string.medication_entry, entry.medicationName!!,
                        entry.medicationDoseAmount!!, entry.medicationDoseUnit!!,
                    )
                entry.diaperKind != null -> stringResource(R.string.diaper, when (entry.diaperKind!!.toInt()) {
                    1 -> stringResource(R.string.wet)
                    2 -> stringResource(R.string.dirty)
                    3 -> stringResource(R.string.both)
                    else -> stringResource(R.string.dry)
                })
                else -> stringResource(R.string.unknown_activity)
            }
            Card(Modifier.fillMaxWidth()) {
                Column(Modifier.padding(12.dp)) {
                    Text(label, fontWeight = FontWeight.SemiBold)
                    Text(DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(entry.startUtcMs)))
                    if (entry.kind != "note" && entry.note != null) {
                        Text(stringResource(R.string.activity_note, entry.note!!))
                    }
                    if (entry.kind == "sleep" && entry.sleepPlace != null) {
                        val placeLabel = when (entry.sleepPlace!!.toInt()) {
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
                        Text(stringResource(if (expandedEntryKey == entryKey)
                            R.string.hide_entry_actions else R.string.show_entry_actions))
                    }
                    if (expandedEntryKey == entryKey) {
                    if (entry.kind == "sleep" && entry.endUtcMs == null) {
                        Button(onClick = { actions.onStopSleep(entry) }) { Text(stringResource(R.string.stop_sleep)) }
                    }
                    if (entry.kind == "sleep" && entry.endUtcMs != null) {
                        OutlinedButton(onClick = { actions.onEditSleep(entry) }) { Text(stringResource(R.string.edit_sleep)) }
                    }
                    if (entry.kind == "sleep") {
                        OutlinedButton(onClick = { actions.onEditSleepPlace(entry) }) { Text(stringResource(R.string.edit_sleep_place)) }
                    }
                    if (entry.kind in noteEditableKinds) {
                        OutlinedButton(onClick = { actions.onAddActivityNote(entry) }) { Text(stringResource(if (entry.note == null)
                            R.string.add_activity_note else R.string.edit_note)) }
                    }
                    if (entry.kind in instantTimeEditableKinds) {
                        OutlinedButton(onClick = { actions.onEditEntryTime(entry) }) { Text(stringResource(R.string.edit_entry_time)) }
                    }
                    if ((entry.kind == "sleep" || entry.kind == "pump") &&
                        entry.endUtcMs != null && entry.endUtcMs!! > entry.startUtcMs) {
                        OutlinedButton(onClick = { actions.onMoveCompletedSession(entry) }) { Text(stringResource(R.string.move_completed_session)) }
                    }
                    if (entry.kind == "feed.bottle" && entry.bottleMl != null) {
                        OutlinedButton(onClick = { actions.onEditBottle(entry) }) { Text(stringResource(R.string.edit_bottle)) }
                    }
                    if (entry.kind == "feed.breast" && entry.breastSegments?.all {
                        (it.endUtcMs - it.startUtcMs) % 60_000L == 0L
                    } == true) {
                        OutlinedButton(onClick = { actions.onEditBreast(entry) }) { Text(stringResource(R.string.edit_breast)) }
                    }
                    if (entry.kind == "diaper" && entry.diaperKind != null) {
                        OutlinedButton(onClick = { actions.onEditDiaper(entry) }) { Text(stringResource(R.string.edit_diaper)) }
                    }
                    if (entry.kind == "feed.solids" && entry.solidsFoods != null) {
                        OutlinedButton(onClick = { actions.onEditSolids(entry) }) { Text(stringResource(R.string.edit_solids)) }
                    }
                    if (entry.kind == "pump") {
                        OutlinedButton(onClick = { actions.onEditPump(entry) }) { Text(stringResource(R.string.edit_pump)) }
                    }
                    if (entry.kind == "medication" && entry.medicationName != null) {
                        OutlinedButton(onClick = { actions.onEditMedication(entry) }) { Text(stringResource(R.string.edit_medication)) }
                    }
                    if (entry.kind == "growth") {
                        OutlinedButton(onClick = { actions.onEditGrowth(entry) }) { Text(stringResource(R.string.edit_growth)) }
                    }
                    if (entry.kind == "temperature" && entry.temperatureC != null) {
                        OutlinedButton(onClick = { actions.onEditTemperature(entry) }) { Text(stringResource(R.string.edit_temperature)) }
                    }
                    OutlinedButton(onClick = { actions.onDeleteEntry(entry) }) { Text(stringResource(R.string.delete_entry)) }
                    }
                }
            }
        }
    }
}
