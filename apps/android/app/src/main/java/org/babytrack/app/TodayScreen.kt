package org.babytrack.app

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import uniffi.babytrack_core_ffi.ActivityRow
import uniffi.babytrack_core_ffi.DaySummaryRow

internal data class TodayUiState(
    val activeShared: Boolean,
    val ageLabel: String,
    val automaticSyncDelayed: Boolean,
    val automaticSyncBlocked: Boolean,
    val summaryIsCurrent: Boolean,
    val daySummary: DaySummaryRow?,
    val entries: List<ActivityRow>,
    val entriesAreCurrent: Boolean,
)

internal data class TodayActions(
    val onStopSleep: (ActivityRow) -> Unit = { _ -> },
    val onStartSleep: () -> Unit = {},
    val onQuickWetDiaper: () -> Unit = {},
    val onOpenBottle: () -> Unit = {},
    val onOpenDiaper: () -> Unit = {},
    val onAddActivity: () -> Unit = {},
    val onViewTimeline: () -> Unit = {},
)

@Composable
internal fun ColumnScope.TodayScreen(state: TodayUiState, actions: TodayActions) {
    with(state) {
        Text(
            stringResource(if (activeShared) R.string.shared_family_short else R.string.local_only),
            style = MaterialTheme.typography.labelMedium,
        )
        Text(ageLabel, style = MaterialTheme.typography.bodyMedium)
        if (automaticSyncDelayed && !automaticSyncBlocked)
            Text(
                stringResource(R.string.automatic_sync_delayed),
                color = MaterialTheme.colorScheme.error,
            )
        if (automaticSyncBlocked)
            Text(
                stringResource(R.string.shared_upload_blocked),
                color = MaterialTheme.colorScheme.error,
            )
        if (summaryIsCurrent && daySummary != null) {
            val today = daySummary!!
            val sleepMinutes = today.sleepMs.toLong() / 60_000L
            val lastFeed =
                entries
                    .filter {
                        it.kind == "feed.breast" ||
                            it.kind == "feed.bottle" ||
                            it.kind == "feed.solids"
                    }
                    .maxByOrNull { it.startUtcMs }
            val lastDiaper = entries.filter { it.kind == "diaper" }.maxByOrNull { it.startUtcMs }
            val runningSleep =
                entries
                    .filter { it.kind == "sleep" && it.endUtcMs == null }
                    .maxByOrNull { it.startUtcMs }
            Card(Modifier.fillMaxWidth()) {
                Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(stringResource(R.string.today_summary), fontWeight = FontWeight.SemiBold)
                    Text(stringResource(R.string.today_sleep, sleepMinutes / 60, sleepMinutes % 60))
                    Text(
                        pluralStringResource(
                            R.plurals.today_feeds,
                            today.feedCount.toInt(),
                            today.feedCount.toLong(),
                            today.bottleMl.toLong(),
                        )
                    )
                    Text(
                        pluralStringResource(
                            R.plurals.today_diapers,
                            today.diaperCount.toInt(),
                            today.diaperCount.toLong(),
                            today.wetDiaperCount.toLong(),
                            today.dirtyDiaperCount.toLong(),
                        )
                    )
                    lastFeed?.let {
                        Text(stringResource(R.string.last_feed, savedTime(it.startUtcMs)))
                    }
                    lastDiaper?.let {
                        Text(stringResource(R.string.last_diaper, savedTime(it.startUtcMs)))
                    }
                    if (runningSleep != null) {
                        val timer = runningSleep
                        Text(
                            stringResource(
                                R.string.running_sleep_since,
                                savedTime(timer.startUtcMs),
                            )
                        )
                        Button(onClick = { actions.onStopSleep(timer) }) {
                            Text(stringResource(R.string.stop_sleep))
                        }
                    } else {
                        Button(onClick = actions.onStartSleep) {
                            Text(stringResource(R.string.start_sleep))
                        }
                    }
                }
            }
        }
        Text(stringResource(R.string.quick_log), style = MaterialTheme.typography.titleMedium)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            val description = stringResource(R.string.quick_wet_diaper_description)
            OutlinedButton(
                onClick = actions.onQuickWetDiaper,
                modifier = Modifier.weight(1f).semantics { contentDescription = description },
            ) {
                Text(stringResource(R.string.quick_wet_diaper))
            }
            OutlinedButton(onClick = actions.onOpenBottle, modifier = Modifier.weight(1f)) {
                Text(stringResource(R.string.event_bottle))
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = actions.onOpenDiaper, modifier = Modifier.weight(1f)) {
                Text(stringResource(R.string.event_diaper))
            }
            Button(onClick = actions.onAddActivity, modifier = Modifier.weight(1f)) {
                Text(stringResource(R.string.add_activity))
            }
        }
        Text(stringResource(R.string.recent_entries), style = MaterialTheme.typography.titleMedium)
        val recentEntries =
            if (entriesAreCurrent) entries.sortedByDescending { it.startUtcMs }.take(3)
            else emptyList()
        if (recentEntries.isEmpty()) Text(stringResource(R.string.no_entries))
        recentEntries.forEach { entry ->
            Card(Modifier.fillMaxWidth()) {
                Column(Modifier.padding(12.dp)) {
                    Text(
                        stringResource(activityLabel(entry.kind)),
                        fontWeight = FontWeight.SemiBold,
                    )
                    Text(savedTime(entry.startUtcMs), style = MaterialTheme.typography.bodySmall)
                }
            }
        }
        TextButton(onClick = actions.onViewTimeline) {
            Text(stringResource(R.string.view_timeline))
        }
    }
}
