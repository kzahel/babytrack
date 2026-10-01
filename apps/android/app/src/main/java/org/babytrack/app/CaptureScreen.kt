package org.babytrack.app

import android.content.Context
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.Air
import androidx.compose.material.icons.outlined.Circle
import androidx.compose.material.icons.outlined.JoinFull
import androidx.compose.material.icons.outlined.Pause
import androidx.compose.material.icons.outlined.PlayArrow
import androidx.compose.material.icons.outlined.Stop
import androidx.compose.material.icons.outlined.Timer
import androidx.compose.material.icons.outlined.Remove
import androidx.compose.material.icons.outlined.WaterDrop
import androidx.compose.material3.AssistChip
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.foundation.clickable
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import java.math.BigDecimal

internal data class CaptureUiState(
    val childName: String,
    val captureKind: CaptureKind?,
    val logAtMs: Long?,
    val amount: String,
    val bottleUnit: UByte,
    val bottleContent: UByte,
    val breastMinutes: String,
    val breastSide: UByte,
    val breastDraftSegments: List<Pair<UByte, Long>>,
    val pumpMinutes: String,
    val pumpLeft: String,
    val pumpRight: String,
    val pumpTotal: String,
    val solidsFoods: String,
    val solidsAmount: String,
    val sleepMinutes: String,
    val sleepPlace: UByte?,
    val growthWeight: String,
    val growthWeightUnit: UByte,
    val growthLength: String,
    val growthLengthUnit: UByte,
    val growthHead: String,
    val growthHeadUnit: UByte,
    val temperatureEntered: String,
    val temperatureUnit: UByte,
    val medicationName: String,
    val doseAmount: String,
    val doseUnit: String,
    val noteText: String,
    val diaperKind: UByte? = null,
    /** The most recent saved bottle, as entered, for one-tap repeat. */
    val lastBottle: Pair<String, UByte>? = null,
    val nowMs: Long = 0L,
    val liveClock: Boolean = false,
    val breastTimerMode: Boolean = true,
    val nursingSegments: List<TimedSegment> = emptyList(),
    /** The side the most recent saved breast feed ended on. */
    val lastBreastSide: UByte? = null,
    val pumpTimerStartMs: Long? = null,
)

internal data class CaptureActions(
    val onCaptureKindChange: (CaptureKind) -> Unit = { _ -> },
    val onLogTimeChoose: () -> Unit = {},
    val onResetLogTime: () -> Unit = {},
    val onDiaperKindChange: (UByte) -> Unit = { _ -> },
    val onLogDiaper: (UByte) -> Unit = { _ -> },
    val onBottleContentChange: (UByte) -> Unit = { _ -> },
    val onBottleUnitChange: (UByte) -> Unit = { _ -> },
    val onAmountChange: (String) -> Unit = { _ -> },
    val onRepeatBottle: (String, UByte) -> Unit = { _, _ -> },
    val onSaveBottle: () -> Unit = {},
    val onBreastModeChange: (Boolean) -> Unit = { _ -> },
    val onNursingTap: (UByte) -> Unit = { _ -> },
    val onDiscardNursing: () -> Unit = {},
    val onPumpTimerStart: () -> Unit = {},
    val onPumpTimerStop: () -> Unit = {},
    val onBreastSideChange: (UByte) -> Unit = { _ -> },
    val onBreastMinutesChange: (String) -> Unit = { _ -> },
    val onRemoveBreastSegment: () -> Unit = {},
    val onAddBreastSegment: () -> Unit = {},
    val onSaveBreast: () -> Unit = {},
    val onPumpMinutesChange: (String) -> Unit = { _ -> },
    val onPumpLeftChange: (String) -> Unit = { _ -> },
    val onPumpRightChange: (String) -> Unit = { _ -> },
    val onPumpTotalChange: (String) -> Unit = { _ -> },
    val onSavePump: () -> Unit = {},
    val onSolidsFoodsChange: (String) -> Unit = { _ -> },
    val onSolidsAmountChange: (String) -> Unit = { _ -> },
    val onSaveSolids: () -> Unit = {},
    val onSleepPlaceChange: (UByte?) -> Unit = { _ -> },
    val onStartSleep: () -> Unit = {},
    val onSleepMinutesChange: (String) -> Unit = { _ -> },
    val onSaveSleep: () -> Unit = {},
    val onGrowthWeightUnitChange: (UByte) -> Unit = { _ -> },
    val onGrowthWeightChange: (String) -> Unit = { _ -> },
    val onGrowthLengthUnitChange: (UByte) -> Unit = { _ -> },
    val onGrowthLengthChange: (String) -> Unit = { _ -> },
    val onGrowthHeadUnitChange: (UByte) -> Unit = { _ -> },
    val onGrowthHeadChange: (String) -> Unit = { _ -> },
    val onSaveGrowth: () -> Unit = {},
    val onTemperatureUnitChange: (UByte) -> Unit = { _ -> },
    val onTemperatureEnteredChange: (String) -> Unit = { _ -> },
    val onSaveTemperature: () -> Unit = {},
    val onMedicationNameChange: (String) -> Unit = { _ -> },
    val onDoseAmountChange: (String) -> Unit = { _ -> },
    val onDoseUnitChange: (String) -> Unit = { _ -> },
    val onSaveMedication: () -> Unit = {},
    val onNoteTextChange: (String) -> Unit = { _ -> },
    val onSaveNote: () -> Unit = {},
)

private val chooserGroups =
    listOf(
        R.string.capture_group_feeding to
            listOf(CaptureKind.BOTTLE, CaptureKind.BREAST, CaptureKind.PUMP, CaptureKind.SOLIDS),
        R.string.capture_group_sleep_diapers to listOf(CaptureKind.SLEEP, CaptureKind.DIAPER),
        R.string.capture_group_health to
            listOf(CaptureKind.GROWTH, CaptureKind.TEMPERATURE, CaptureKind.MEDICATION),
        R.string.capture_group_notes to listOf(CaptureKind.NOTE),
    )

internal val diaperChoices =
    listOf(
        Triple(1u.toUByte(), Icons.Outlined.WaterDrop, R.string.wet),
        Triple(2u.toUByte(), Icons.Outlined.Circle, R.string.dirty),
        Triple(3u.toUByte(), Icons.Outlined.JoinFull, R.string.both),
        Triple(4u.toUByte(), Icons.Outlined.Air, R.string.dry),
    )

internal val bottleContents =
    listOf(
        1u.toUByte() to R.string.bottle_breast_milk,
        2u.toUByte() to R.string.bottle_formula,
        3u.toUByte() to R.string.bottle_mixed,
        4u.toUByte() to R.string.bottle_other,
    )

internal val bottleUnits =
    listOf(
        1u.toUByte() to R.string.unit_ml,
        2u.toUByte() to R.string.unit_us_fl_oz,
        3u.toUByte() to R.string.unit_uk_fl_oz,
    )

internal val sleepPlaces =
    listOf(
        null to R.string.sleep_place_unspecified,
        1u.toUByte() to R.string.sleep_place_crib,
        2u.toUByte() to R.string.sleep_place_pram,
        3u.toUByte() to R.string.sleep_place_contact,
        4u.toUByte() to R.string.sleep_place_car,
        5u.toUByte() to R.string.sleep_place_other,
    )

@Composable
internal fun ColumnScope.CaptureScreen(state: CaptureUiState, actions: CaptureActions) {
    with(state) {
        if (captureKind == null) {
            ActivityChooser(actions.onCaptureKindChange)
            return
        }
        if (!(captureKind == CaptureKind.BREAST && breastTimerMode)) TimeRow(
            value =
                if (logAtMs == null) stringResource(R.string.log_time_now_short)
                else compactDateTime(LocalContext.current, logAtMs!!, nowMs),
            chooseLabel = stringResource(R.string.log_time_change),
            onChoose = actions.onLogTimeChoose,
            resetLabel = if (logAtMs != null) stringResource(R.string.log_time_use_now) else null,
            onReset = actions.onResetLogTime,
        )
        when (captureKind) {
            CaptureKind.DIAPER -> DiaperForm(state, actions)
            CaptureKind.BOTTLE -> BottleForm(state, actions)
            CaptureKind.BREAST -> BreastForm(state, actions)
            CaptureKind.PUMP -> PumpForm(state, actions)
            CaptureKind.SOLIDS -> SolidsForm(state, actions)
            CaptureKind.SLEEP -> SleepForm(state, actions)
            CaptureKind.GROWTH -> GrowthForm(state, actions)
            CaptureKind.TEMPERATURE -> TemperatureForm(state, actions)
            CaptureKind.MEDICATION -> MedicationForm(state, actions)
            CaptureKind.NOTE -> NoteForm(state, actions)
        }
    }
}

/** The bottom bar's actions for the open form; the chooser has none. */
@Composable
internal fun RowScope.CaptureSaveActions(state: CaptureUiState, actions: CaptureActions) {
    with(state) {
        when (captureKind) {
            null -> Unit
            CaptureKind.DIAPER ->
                PrimaryAction(stringResource(R.string.save_diaper), enabled = diaperKind != null) {
                    diaperKind?.let(actions.onLogDiaper)
                }
            CaptureKind.BOTTLE ->
                PrimaryAction(
                    stringResource(R.string.save_bottle),
                    enabled = validBottleAmount(amount, bottleUnit),
                    onClick = actions.onSaveBottle,
                )
            CaptureKind.BREAST ->
                PrimaryAction(
                    stringResource(R.string.save_breast),
                    enabled = if (breastTimerMode) nursingSegments.isNotEmpty() else breastCanSave(state),
                    onClick = actions.onSaveBreast,
                )
            CaptureKind.PUMP ->
                PrimaryAction(
                    stringResource(R.string.save_pump),
                    enabled = pumpCanSave(state),
                    onClick = actions.onSavePump,
                )
            CaptureKind.SOLIDS ->
                PrimaryAction(
                    stringResource(R.string.save_solids),
                    enabled = solidsFoods.isNotBlank(),
                    onClick = actions.onSaveSolids,
                )
            CaptureKind.SLEEP -> {
                OutlinedButton(
                    onClick = actions.onStartSleep,
                    modifier = Modifier.weight(1f).heightIn(min = 56.dp),
                ) {
                    Text(stringResource(R.string.start_sleep), textAlign = TextAlign.Center)
                }
                PrimaryAction(
                    stringResource(R.string.save_sleep),
                    enabled = (sleepMinutes.toLongOrNull() ?: 0L) in 1L..1440L,
                    onClick = actions.onSaveSleep,
                )
            }
            CaptureKind.GROWTH ->
                PrimaryAction(
                    stringResource(R.string.save_growth),
                    enabled =
                        listOf(growthWeight, growthLength, growthHead).any { it.isNotBlank() } &&
                            validGrowthAmount(growthWeight, growthWeightUnit) &&
                            validGrowthAmount(growthLength, growthLengthUnit) &&
                            validGrowthAmount(growthHead, growthHeadUnit),
                    onClick = actions.onSaveGrowth,
                )
            CaptureKind.TEMPERATURE ->
                PrimaryAction(
                    stringResource(R.string.save_temperature),
                    enabled = validTemperature(temperatureEntered),
                    onClick = actions.onSaveTemperature,
                )
            CaptureKind.MEDICATION ->
                PrimaryAction(
                    stringResource(R.string.save_medication),
                    enabled =
                        medicationName.isNotBlank() && doseAmount.isNotBlank() && doseUnit.isNotBlank(),
                    onClick = actions.onSaveMedication,
                )
            CaptureKind.NOTE ->
                PrimaryAction(
                    stringResource(R.string.save_note),
                    enabled = noteText.isNotBlank(),
                    onClick = actions.onSaveNote,
                )
        }
    }
}

private fun breastCanSave(state: CaptureUiState): Boolean {
    val next = state.breastMinutes.toLongOrNull()
    val total = state.breastDraftSegments.sumOf { it.second } + (next ?: 0L)
    return next != null && next in 1L..240L && state.breastDraftSegments.size < 8 && total <= 240L
}

private fun pumpCanSave(state: CaptureUiState): Boolean =
    with(state) {
        val duration = pumpMinutes.toLongOrNull() ?: 0L
        val sides = (pumpLeft.toLongOrNull() ?: 0L) + (pumpRight.toLongOrNull() ?: 0L)
        duration in 1L..240L &&
            ((pumpTotal.isBlank() && sides > 0L) ||
                (pumpLeft.isBlank() && pumpRight.isBlank() && (pumpTotal.toLongOrNull() ?: 0L) > 0L))
    }

@Composable
private fun ColumnScope.ActivityChooser(onSelect: (CaptureKind) -> Unit) {
    chooserGroups.forEach { (heading, kinds) ->
        SectionHeader(stringResource(heading))
        kinds.chunked(2).forEach { row ->
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                row.forEach { kind -> ChooserTile(kind, Modifier.weight(1f)) { onSelect(kind) } }
                if (row.size == 1) Column(Modifier.weight(1f)) {}
            }
        }
    }
}

@Composable
private fun ChooserTile(kind: CaptureKind, modifier: Modifier, onClick: () -> Unit) {
    val colors = categoryColors(kind.activityKind)
    Surface(
        shape = MaterialTheme.shapes.medium,
        color = colors.container,
        modifier = modifier.heightIn(min = 72.dp).clip(MaterialTheme.shapes.medium).clickable(onClick = onClick),
    ) {
        Row(
            Modifier.padding(horizontal = 14.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Icon(
                activityIcon(kind.activityKind),
                contentDescription = null,
                tint = colors.accent,
                modifier = Modifier.size(28.dp),
            )
            Text(stringResource(kind.label), style = MaterialTheme.typography.titleSmall)
        }
    }
}

@Composable
private fun FieldLabel(id: Int) {
    Text(
        stringResource(id),
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

@Composable
private fun DiaperForm(state: CaptureUiState, actions: CaptureActions) {
    ChoiceTiles(
        diaperChoices.map { (kind, icon, label) -> Triple(kind, icon, stringResource(label)) },
        selected = state.diaperKind,
        onSelect = actions.onDiaperKindChange,
    )
}

private fun bottleStep(unit: UByte): BigDecimal =
    if (unit == 1u.toUByte()) BigDecimal(10) else BigDecimal("0.5")

/** The amount after one stepper press, in the draft's display form. */
internal fun steppedBottleAmount(context: Context, amount: String, unit: UByte, up: Boolean): String {
    val current = canonicalDecimal(amount).toBigDecimalOrNull() ?: BigDecimal.ZERO
    val step = bottleStep(unit)
    val next = if (up) current + step else (current - step).max(BigDecimal.ZERO)
    return if (next.signum() == 0) "" else localizedEntered(context, next.stripTrailingZeros().toPlainString())
}

@Composable
private fun BottleForm(state: CaptureUiState, actions: CaptureActions) {
    val context = LocalContext.current
    with(state) {
        FieldLabel(R.string.bottle_content_title)
        ChipRow(
            bottleContents.map { (value, label) -> value to stringResource(label) },
            bottleContent,
            actions.onBottleContentChange,
        )
        FieldLabel(R.string.bottle_unit_title)
        SegmentedChoice(
            bottleUnits.map { (value, label) -> value to stringResource(label) },
            bottleUnit,
            actions.onBottleUnitChange,
        )
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            FilledTonalIconButton(
                onClick = { actions.onAmountChange(steppedBottleAmount(context, amount, bottleUnit, false)) },
                modifier = Modifier.size(56.dp),
            ) {
                Icon(Icons.Outlined.Remove, contentDescription = stringResource(R.string.bottle_less))
            }
            OutlinedTextField(
                value = amount,
                onValueChange = { it -> actions.onAmountChange(it) },
                label = { Text(stringResource(R.string.bottle_amount)) },
                suffix = { Text(stringResource(bottleUnits.first { it.first == bottleUnit }.second)) },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                textStyle = MaterialTheme.typography.headlineSmall,
                modifier = Modifier.weight(1f),
                singleLine = true,
            )
            FilledTonalIconButton(
                onClick = { actions.onAmountChange(steppedBottleAmount(context, amount, bottleUnit, true)) },
                modifier = Modifier.size(56.dp),
            ) {
                Icon(Icons.Outlined.Add, contentDescription = stringResource(R.string.bottle_more))
            }
        }
        lastBottle?.let { (entered, unit) ->
            val shown =
                stringResource(
                    R.string.bottle_repeat,
                    localizedEntered(context, entered),
                    stringResource(bottleUnits.firstOrNull { it.first == unit }?.second ?: R.string.unit_ml),
                )
            AssistChip(onClick = { actions.onRepeatBottle(entered, unit) }, label = { Text(shown) })
        }
    }
}

@Composable
private fun ColumnScope.BreastForm(state: CaptureUiState, actions: CaptureActions) {
    SegmentedChoice(
        listOf(
            true to stringResource(R.string.breast_mode_timer),
            false to stringResource(R.string.breast_mode_manual),
        ),
        state.breastTimerMode,
        actions.onBreastModeChange,
    )
    if (state.breastTimerMode) NursingTimer(state, actions) else ManualBreastForm(state, actions)
}

@Composable
private fun ColumnScope.NursingTimer(state: CaptureUiState, actions: CaptureActions) {
    val context = LocalContext.current
    with(state) {
        val running = nursingSegments.running()
        val now = rememberNow(nowMs, liveClock && running != null, 1_000L)
        Column(
            Modifier.fillMaxWidth().padding(vertical = 8.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(
                elapsedClock(totalElapsedMs(nursingSegments, now)),
                style = MaterialTheme.typography.displayLarge,
            )
            Text(
                stringResource(
                    when {
                        running != null -> R.string.timer_running
                        nursingSegments.isNotEmpty() -> R.string.timer_paused
                        else -> R.string.timer_choose_side
                    }
                ),
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            listOf(1u.toUByte() to R.string.breast_left, 2u.toUByte() to R.string.breast_right).forEach {
                (side, label) ->
                SideTimerButton(
                    label = stringResource(label),
                    elapsed = elapsedClock(sideElapsedMs(nursingSegments, side, now)),
                    running = running?.side == side,
                    otherRunning = running != null && running.side != side,
                    lastSide = nursingSegments.isEmpty() && lastBreastSide == side,
                    enabled = running?.side == side || nursingSegments.size < maxBreastSegments,
                    modifier = Modifier.weight(1f),
                ) {
                    actions.onNursingTap(side)
                }
            }
        }
        if (nursingSegments.isEmpty() && lastBreastSide != null)
            Text(
                stringResource(
                    R.string.breast_last_side,
                    context.getString(
                        if (lastBreastSide == 1u.toUByte()) R.string.breast_left else R.string.breast_right
                    ),
                ),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.align(Alignment.CenterHorizontally),
            )
        if (nursingSegments.isNotEmpty())
            TextButton(
                onClick = actions.onDiscardNursing,
                modifier = Modifier.align(Alignment.CenterHorizontally),
            ) {
                Text(stringResource(R.string.timer_discard))
            }
    }
}

@Composable
private fun SideTimerButton(
    label: String,
    elapsed: String,
    running: Boolean,
    otherRunning: Boolean,
    lastSide: Boolean,
    enabled: Boolean,
    modifier: Modifier,
    onClick: () -> Unit,
) {
    val colors = categoryColors("feed.breast")
    val state =
        stringResource(
            when {
                running -> R.string.timer_tap_pause
                otherRunning -> R.string.timer_tap_switch
                else -> R.string.timer_tap_start
            }
        )
    Surface(
        onClick = onClick,
        enabled = enabled,
        shape = MaterialTheme.shapes.large,
        color = if (running) MaterialTheme.colorScheme.primary else colors.container,
        contentColor =
            if (running) MaterialTheme.colorScheme.onPrimary else MaterialTheme.colorScheme.onSurface,
        modifier = modifier.heightIn(min = 148.dp),
    ) {
        Column(
            Modifier.padding(16.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(4.dp, Alignment.CenterVertically),
        ) {
            Icon(
                if (running) Icons.Outlined.Pause else Icons.Outlined.PlayArrow,
                contentDescription = null,
                modifier = Modifier.size(32.dp),
            )
            Text(label, style = MaterialTheme.typography.titleLarge)
            Text(elapsed, style = MaterialTheme.typography.titleMedium)
            Text(
                if (lastSide) stringResource(R.string.breast_last_side_short) else state,
                style = MaterialTheme.typography.labelMedium,
                textAlign = TextAlign.Center,
            )
        }
    }
}

@Composable
private fun ManualBreastForm(state: CaptureUiState, actions: CaptureActions) {
    val context = LocalContext.current
    with(state) {
        FieldLabel(R.string.breast_side_title)
        SegmentedChoice(
            listOf(1u.toUByte() to stringResource(R.string.breast_left), 2u.toUByte() to stringResource(R.string.breast_right)),
            breastSide,
            actions.onBreastSideChange,
        )
        OutlinedTextField(
            value = breastMinutes,
            onValueChange = { it -> actions.onBreastMinutesChange(it) },
            label = { Text(stringResource(R.string.breast_minutes)) },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
        )
        if (breastDraftSegments.isNotEmpty()) {
            Text(
                stringResource(
                    R.string.breast_segments_draft,
                    breastDraftSegments.joinToString(" → ") { (side, minutes) ->
                        context.getString(
                            R.string.breast_segment_summary,
                            context.getString(
                                if (side == 1u.toUByte()) R.string.breast_left else R.string.breast_right
                            ),
                            minutes,
                        )
                    },
                ),
                style = MaterialTheme.typography.bodyLarge,
            )
        }
        val next = breastMinutes.toLongOrNull()
        val total = breastDraftSegments.sumOf { it.second } + (next ?: 0L)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(
                enabled = next != null && next in 1L..240L && breastDraftSegments.size < 7 && total <= 240L,
                onClick = actions.onAddBreastSegment,
            ) {
                Text(stringResource(R.string.add_breast_segment))
            }
            if (breastDraftSegments.isNotEmpty())
                TextButton(onClick = actions.onRemoveBreastSegment) {
                    Text(stringResource(R.string.remove_last_segment))
                }
        }
    }
}

@Composable
private fun PumpForm(state: CaptureUiState, actions: CaptureActions) {
    with(state) {
        val start = pumpTimerStartMs
        if (start != null) {
            val now = rememberNow(nowMs, liveClock, 1_000L)
            Surface(
                shape = MaterialTheme.shapes.large,
                color = categoryColors("pump").container,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Column(
                    Modifier.padding(16.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Text(elapsedClock(now - start), style = MaterialTheme.typography.displayMedium)
                    Text(
                        stringResource(R.string.pump_timer_running),
                        style = MaterialTheme.typography.bodyLarge,
                    )
                    androidx.compose.material3.Button(
                        onClick = actions.onPumpTimerStop,
                        modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp),
                    ) {
                        Icon(Icons.Outlined.Stop, contentDescription = null)
                        Text(
                            stringResource(R.string.pump_timer_stop),
                            modifier = Modifier.padding(start = 8.dp),
                        )
                    }
                }
            }
        } else
            OutlinedButton(
                onClick = actions.onPumpTimerStart,
                modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp),
            ) {
                Icon(Icons.Outlined.Timer, contentDescription = null)
                Text(
                    stringResource(R.string.pump_timer_start),
                    modifier = Modifier.padding(start = 8.dp),
                )
            }
        OutlinedTextField(
            value = pumpMinutes,
            onValueChange = { it -> actions.onPumpMinutesChange(it) },
            label = { Text(stringResource(R.string.pump_minutes)) },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
        )
        FieldLabel(R.string.pump_amount_title)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedTextField(
                value = pumpLeft,
                onValueChange = { it -> actions.onPumpLeftChange(it) },
                label = { Text(stringResource(R.string.pump_left_ml)) },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                modifier = Modifier.weight(1f),
                singleLine = true,
            )
            OutlinedTextField(
                value = pumpRight,
                onValueChange = { it -> actions.onPumpRightChange(it) },
                label = { Text(stringResource(R.string.pump_right_ml)) },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                modifier = Modifier.weight(1f),
                singleLine = true,
            )
        }
        OutlinedTextField(
            value = pumpTotal,
            onValueChange = { it -> actions.onPumpTotalChange(it) },
            label = { Text(stringResource(R.string.pump_total_ml)) },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
        )
    }
}

@Composable
private fun SolidsForm(state: CaptureUiState, actions: CaptureActions) {
    OutlinedTextField(
        value = state.solidsFoods,
        onValueChange = { it -> actions.onSolidsFoodsChange(it) },
        label = { Text(stringResource(R.string.solids_foods)) },
        minLines = 3,
        modifier = Modifier.fillMaxWidth(),
    )
    OutlinedTextField(
        value = state.solidsAmount,
        onValueChange = { it -> actions.onSolidsAmountChange(it) },
        label = { Text(stringResource(R.string.solids_amount)) },
        modifier = Modifier.fillMaxWidth(),
    )
}

@Composable
private fun SleepForm(state: CaptureUiState, actions: CaptureActions) {
    OutlinedTextField(
        value = state.sleepMinutes,
        onValueChange = { it -> actions.onSleepMinutesChange(it) },
        label = { Text(stringResource(R.string.sleep_minutes)) },
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
        modifier = Modifier.fillMaxWidth(),
        singleLine = true,
    )
    FieldLabel(R.string.sleep_place_title)
    ChipRow(
        sleepPlaces.map { (place, label) -> place to stringResource(label) },
        state.sleepPlace,
        actions.onSleepPlaceChange,
    )
}

@Composable
private fun GrowthForm(state: CaptureUiState, actions: CaptureActions) {
    with(state) {
        OutlinedTextField(
            value = growthWeight,
            onValueChange = { it -> actions.onGrowthWeightChange(it) },
            label = { Text(stringResource(R.string.weight)) },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
        )
        GrowthUnitChoices(R.string.weight_unit, massUnits, growthWeightUnit, actions.onGrowthWeightUnitChange)
        OutlinedTextField(
            value = growthLength,
            onValueChange = { it -> actions.onGrowthLengthChange(it) },
            label = { Text(stringResource(R.string.length)) },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
        )
        GrowthUnitChoices(R.string.length_unit, lengthUnits, growthLengthUnit, actions.onGrowthLengthUnitChange)
        OutlinedTextField(
            value = growthHead,
            onValueChange = { it -> actions.onGrowthHeadChange(it) },
            label = { Text(stringResource(R.string.head_circumference)) },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
        )
        GrowthUnitChoices(R.string.head_unit, lengthUnits, growthHeadUnit, actions.onGrowthHeadUnitChange)
    }
}

@Composable
private fun TemperatureForm(state: CaptureUiState, actions: CaptureActions) {
    with(state) {
        SegmentedChoice(
            listOf(
                30u.toUByte() to stringResource(R.string.unit_celsius),
                31u.toUByte() to stringResource(R.string.unit_fahrenheit),
            ),
            temperatureUnit,
            actions.onTemperatureUnitChange,
        )
        OutlinedTextField(
            value = temperatureEntered,
            onValueChange = { it -> actions.onTemperatureEnteredChange(it) },
            label = {
                Text(
                    stringResource(
                        if (temperatureUnit == 30u.toUByte()) R.string.temperature_c
                        else R.string.temperature_f
                    )
                )
            },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
            textStyle = MaterialTheme.typography.headlineSmall,
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
        )
    }
}

@Composable
private fun MedicationForm(state: CaptureUiState, actions: CaptureActions) {
    with(state) {
        OutlinedTextField(
            value = medicationName,
            onValueChange = { it -> actions.onMedicationNameChange(it) },
            label = { Text(stringResource(R.string.medication_name)) },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
        )
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedTextField(
                value = doseAmount,
                onValueChange = { it -> actions.onDoseAmountChange(it) },
                label = { Text(stringResource(R.string.dose_amount)) },
                modifier = Modifier.weight(1f),
                singleLine = true,
            )
            OutlinedTextField(
                value = doseUnit,
                onValueChange = { it -> actions.onDoseUnitChange(it) },
                label = { Text(stringResource(R.string.dose_unit)) },
                modifier = Modifier.weight(1f),
                singleLine = true,
            )
        }
    }
}

@Composable
private fun NoteForm(state: CaptureUiState, actions: CaptureActions) {
    OutlinedTextField(
        value = state.noteText,
        onValueChange = { it -> actions.onNoteTextChange(it) },
        label = { Text(stringResource(R.string.note_text)) },
        minLines = 4,
        modifier = Modifier.fillMaxWidth(),
    )
}
