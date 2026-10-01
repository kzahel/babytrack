package org.babytrack.app

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.CloudDone
import androidx.compose.material.icons.outlined.PhoneAndroid
import androidx.compose.material.icons.outlined.Warning
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
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
    /** The instant displayed state is computed from; fixtures fix it. */
    val nowMs: Long,
    /** Whether elapsed labels keep advancing on screen. */
    val liveClock: Boolean = false,
)

internal data class TodayActions(
    val onStopSleep: (ActivityRow) -> Unit = { _ -> },
    val onStartSleep: () -> Unit = {},
    val onOpenSleep: () -> Unit = {},
    val onQuickWetDiaper: () -> Unit = {},
    val onOpenBottle: () -> Unit = {},
    val onOpenBreast: () -> Unit = {},
    val onOpenDiaper: () -> Unit = {},
    val onAddActivity: () -> Unit = {},
    val onViewTimeline: () -> Unit = {},
)

private val feedKinds = setOf("feed.breast", "feed.bottle", "feed.solids")

@Composable
internal fun ColumnScope.TodayScreen(state: TodayUiState, actions: TodayActions) {
    val context = LocalContext.current
    with(state) {
        val current = if (entriesAreCurrent) entries else emptyList()
        val runningSleep =
            current.filter { it.kind == "sleep" && it.endUtcMs == null }.maxByOrNull { it.startUtcMs }
        val now = rememberNow(nowMs, liveClock, if (runningSleep != null) 1_000L else 30_000L)
        val lastFeed = current.filter { it.kind in feedKinds }.maxByOrNull { it.startUtcMs }
        val lastDiaper = current.filter { it.kind == "diaper" }.maxByOrNull { it.startUtcMs }
        val lastSleep =
            current
                .filter { it.kind == "sleep" && it.endUtcMs != null }
                .maxByOrNull { it.endUtcMs!! }

        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                ageLabel,
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.weight(1f),
            )
            StatusChip(
                stringResource(if (activeShared) R.string.shared_family_short else R.string.local_only),
                if (activeShared) Icons.Outlined.CloudDone else Icons.Outlined.PhoneAndroid,
            )
        }
        if (automaticSyncDelayed && !automaticSyncBlocked)
            WarningCard(stringResource(R.string.automatic_sync_delayed))
        if (automaticSyncBlocked) WarningCard(stringResource(R.string.shared_upload_blocked))

        StateTile(
            kind = "feed.bottle",
            title = stringResource(R.string.tile_feed),
            elapsed = lastFeed?.let { elapsedLabel(context, it.startUtcMs, now) },
            detail =
                lastFeed?.let {
                    stringResource(
                        R.string.tile_detail_at,
                        entrySummary(context, it),
                        compactDateTime(context, it.startUtcMs, now),
                    )
                } ?: stringResource(R.string.tile_feed_none),
        ) {
            TileButton(stringResource(R.string.event_bottle), primary = true, onClick = actions.onOpenBottle)
            TileButton(stringResource(R.string.event_breast), primary = false, onClick = actions.onOpenBreast)
        }

        if (runningSleep != null) {
            StateTile(
                kind = "sleep",
                title = stringResource(R.string.tile_sleeping),
                elapsed = null,
                detail =
                    stringResource(
                        R.string.running_sleep_since,
                        compactDateTime(context, runningSleep.startUtcMs, now),
                    ),
                clock = elapsedClock(now - runningSleep.startUtcMs),
            ) {
                TileButton(stringResource(R.string.stop_sleep), primary = true) {
                    actions.onStopSleep(runningSleep)
                }
            }
        } else {
            StateTile(
                kind = "sleep",
                title = stringResource(R.string.tile_sleep),
                elapsed = lastSleep?.let { elapsedLabel(context, it.endUtcMs!!, now) },
                detail =
                    lastSleep?.let {
                        stringResource(
                            R.string.tile_sleep_ended,
                            durationLabel(context, it.endUtcMs!! - it.startUtcMs),
                            compactDateTime(context, it.endUtcMs!!, now),
                        )
                    } ?: stringResource(R.string.tile_sleep_none),
            ) {
                TileButton(stringResource(R.string.start_sleep), primary = true, onClick = actions.onStartSleep)
                TileButton(stringResource(R.string.tile_past_sleep), primary = false, onClick = actions.onOpenSleep)
            }
        }

        StateTile(
            kind = "diaper",
            title = stringResource(R.string.tile_diaper),
            elapsed = lastDiaper?.let { elapsedLabel(context, it.startUtcMs, now) },
            detail =
                lastDiaper?.let {
                    stringResource(
                        R.string.tile_detail_at,
                        entrySummary(context, it),
                        compactDateTime(context, it.startUtcMs, now),
                    )
                } ?: stringResource(R.string.tile_diaper_none),
        ) {
            val description = stringResource(R.string.quick_wet_diaper_description)
            TileButton(
                stringResource(R.string.quick_wet_diaper),
                primary = true,
                modifier = Modifier.semantics { contentDescription = description },
                onClick = actions.onQuickWetDiaper,
            )
            TileButton(stringResource(R.string.log_diaper), primary = false, onClick = actions.onOpenDiaper)
        }

        OutlinedButton(
            onClick = actions.onAddActivity,
            modifier = Modifier.fillMaxWidth().heightIn(min = 52.dp),
        ) {
            Icon(Icons.Outlined.Add, contentDescription = null, modifier = Modifier.size(20.dp))
            Text(
                stringResource(R.string.add_activity),
                modifier = Modifier.padding(start = 8.dp),
            )
        }

        if (summaryIsCurrent && daySummary != null) {
            val today = daySummary!!
            val hasActivity =
                today.feedCount > 0uL || today.diaperCount > 0uL || today.sleepMs > 0uL
            SectionHeader(stringResource(R.string.today_so_far))
            if (!hasActivity && runningSleep == null) {
                Text(
                    stringResource(R.string.today_empty),
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else {
                SectionCard {
                    SummaryLine(
                        "feed.bottle",
                        pluralStringResource(
                            R.plurals.today_feeds,
                            today.feedCount.toInt(),
                            today.feedCount.toLong(),
                            today.bottleMl.toLong(),
                        ),
                    )
                    SummaryLine(
                        "sleep",
                        stringResource(
                            R.string.today_sleep_total,
                            durationLabel(context, today.sleepMs.toLong()),
                        ),
                    )
                    SummaryLine(
                        "diaper",
                        pluralStringResource(
                            R.plurals.today_diapers,
                            today.diaperCount.toInt(),
                            today.diaperCount.toLong(),
                            today.wetDiaperCount.toLong(),
                            today.dirtyDiaperCount.toLong(),
                        ),
                    )
                }
            }
        }

        SectionHeader(
            stringResource(R.string.recent_entries),
            actionLabel = stringResource(R.string.view_timeline),
            onAction = actions.onViewTimeline,
        )
        val recentEntries = current.sortedByDescending { it.startUtcMs }.take(3)
        if (recentEntries.isEmpty())
            Text(
                stringResource(R.string.no_entries),
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        else
            SectionCard {
                recentEntries.forEach { entry ->
                    EntryRow(
                        kind = entry.kind,
                        title = entrySummary(context, entry),
                        detail = null,
                        time = compactDateTime(context, entry.startUtcMs, now),
                        onClick = actions.onViewTimeline,
                    )
                }
            }
    }
}

/** A home tile for one activity group: its last state and the actions that change it. */
@Composable
private fun StateTile(
    kind: String,
    title: String,
    elapsed: String?,
    detail: String,
    clock: String? = null,
    buttons: @Composable RowScope.() -> Unit,
) {
    val colors = categoryColors(kind)
    Surface(
        shape = MaterialTheme.shapes.large,
        color = colors.container,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            Modifier.padding(horizontal = 16.dp, vertical = 14.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                Icon(
                    activityIcon(kind),
                    contentDescription = null,
                    tint = colors.accent,
                    modifier = Modifier.size(24.dp),
                )
                Text(
                    title,
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.weight(1f),
                )
                if (elapsed != null)
                    Text(
                        elapsed,
                        style = MaterialTheme.typography.titleSmall,
                        color = colors.accent,
                        textAlign = TextAlign.End,
                    )
            }
            if (clock != null)
                Text(
                    clock,
                    style = MaterialTheme.typography.displayMedium,
                    fontWeight = FontWeight.Medium,
                )
            Text(detail, style = MaterialTheme.typography.bodyLarge)
            Row(
                Modifier.padding(top = 4.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                content = buttons,
            )
        }
    }
}

@Composable
private fun RowScope.TileButton(
    text: String,
    primary: Boolean,
    modifier: Modifier = Modifier,
    onClick: () -> Unit,
) {
    val sized = modifier.weight(1f).heightIn(min = 48.dp)
    if (primary)
        Button(onClick = onClick, modifier = sized) { Text(text, textAlign = TextAlign.Center) }
    else
        OutlinedButton(onClick = onClick, modifier = sized) {
            Text(text, textAlign = TextAlign.Center)
        }
}

@Composable
private fun SummaryLine(kind: String, text: String) {
    Row(
        Modifier.fillMaxWidth().padding(vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        ActivityBadge(kind, size = 32.dp)
        Text(text, style = MaterialTheme.typography.bodyLarge)
    }
}

@Composable
internal fun WarningCard(text: String) {
    Surface(
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.errorContainer,
        contentColor = MaterialTheme.colorScheme.onErrorContainer,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(
            Modifier.padding(16.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Icon(Icons.Outlined.Warning, contentDescription = null)
            Text(text, style = MaterialTheme.typography.bodyMedium)
        }
    }
}
