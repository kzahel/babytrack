package org.babytrack.app

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import java.text.DateFormat
import java.util.Date

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
)

internal data class CaptureActions(
    val onCaptureKindChange: (CaptureKind) -> Unit = { _ -> },
    val onLogTimeChoose: () -> Unit = {},
    val onResetLogTime: () -> Unit = {},
    val onLogDiaper: (UByte) -> Unit = { _ -> },
    val onBottleContentChange: (UByte) -> Unit = { _ -> },
    val onBottleUnitChange: (UByte) -> Unit = { _ -> },
    val onAmountChange: (String) -> Unit = { _ -> },
    val onSaveBottle: () -> Unit = {},
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

@Composable
internal fun ColumnScope.CaptureScreen(state: CaptureUiState, actions: CaptureActions) {
    val context = LocalContext.current
    with(state) {
        if (captureKind == null) {
            Text(stringResource(R.string.add_activity), style = MaterialTheme.typography.titleLarge)
            CaptureKind.entries.chunked(2).forEach { kinds ->
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    kinds.forEach { kind ->
                        OutlinedButton(
                            onClick = { actions.onCaptureKindChange(kind) },
                            modifier = Modifier.weight(1f).heightIn(min = 88.dp),
                        ) {
                            Text(stringResource(kind.label), textAlign = TextAlign.Center)
                        }
                    }
                }
            }
        } else {
            Text(
                stringResource(R.string.capture_for_child, childName),
                style = MaterialTheme.typography.titleMedium,
            )
            Text(
                stringResource(R.string.log_time_title),
                style = MaterialTheme.typography.titleMedium,
            )
            Text(
                if (logAtMs == null) stringResource(R.string.log_time_now)
                else
                    stringResource(
                        R.string.log_time_selected,
                        DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT)
                            .format(Date(logAtMs!!)),
                    )
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedButton(onClick = actions.onLogTimeChoose) {
                    Text(stringResource(R.string.log_time_choose))
                }
                if (logAtMs != null)
                    OutlinedButton(onClick = actions.onResetLogTime) {
                        Text(stringResource(R.string.log_time_reset))
                    }
            }
            if (captureKind == CaptureKind.DIAPER) {
                Text(
                    stringResource(R.string.log_diaper),
                    style = MaterialTheme.typography.titleLarge,
                )
                listOf(
                        1u.toUByte() to R.string.wet,
                        2u.toUByte() to R.string.dirty,
                        3u.toUByte() to R.string.both,
                        4u.toUByte() to R.string.dry,
                    )
                    .chunked(2)
                    .forEach { options ->
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            options.forEach { (kind, label) ->
                                Button(onClick = { actions.onLogDiaper(kind) }) {
                                    Text(stringResource(label))
                                }
                            }
                        }
                    }
            }
            if (captureKind == CaptureKind.BOTTLE) {
                Text(
                    stringResource(R.string.log_bottle),
                    style = MaterialTheme.typography.titleLarge,
                )
                listOf(
                        1u.toUByte() to R.string.bottle_breast_milk,
                        2u.toUByte() to R.string.bottle_formula,
                        3u.toUByte() to R.string.bottle_mixed,
                        4u.toUByte() to R.string.bottle_other,
                    )
                    .chunked(2)
                    .forEach { options ->
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            options.forEach { (content, label) ->
                                FilterChip(
                                    selected = bottleContent == content,
                                    onClick = { actions.onBottleContentChange(content) },
                                    label = { Text(stringResource(label)) },
                                )
                            }
                        }
                    }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    listOf(
                            1u.toUByte() to R.string.unit_ml,
                            2u.toUByte() to R.string.unit_us_fl_oz,
                            3u.toUByte() to R.string.unit_uk_fl_oz,
                        )
                        .forEach { (unit, label) ->
                            FilterChip(
                                selected = bottleUnit == unit,
                                onClick = { actions.onBottleUnitChange(unit) },
                                label = { Text(stringResource(label)) },
                            )
                        }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedTextField(
                        value = amount,
                        onValueChange = { it -> actions.onAmountChange(it) },
                        label = { Text(stringResource(R.string.bottle_amount)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                        modifier = Modifier.weight(1f),
                        singleLine = true,
                    )
                    Button(
                        enabled = validBottleAmount(amount, bottleUnit),
                        onClick = actions.onSaveBottle,
                    ) {
                        Text(stringResource(R.string.log_bottle))
                    }
                }
            }
            if (captureKind == CaptureKind.BREAST) {
                Text(
                    stringResource(R.string.log_breast),
                    style = MaterialTheme.typography.titleLarge,
                )
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    listOf(
                            1u.toUByte() to R.string.breast_left,
                            2u.toUByte() to R.string.breast_right,
                        )
                        .forEach { (side, label) ->
                            FilterChip(
                                selected = breastSide == side,
                                onClick = { actions.onBreastSideChange(side) },
                                label = { Text(stringResource(label)) },
                            )
                        }
                }
                OutlinedTextField(
                    value = breastMinutes,
                    onValueChange = { it -> actions.onBreastMinutesChange(it) },
                    label = { Text(stringResource(R.string.breast_minutes)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
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
                                        if (side == 1u.toUByte()) R.string.breast_left
                                        else R.string.breast_right
                                    ),
                                    minutes,
                                )
                            },
                        )
                    )
                    OutlinedButton(onClick = actions.onRemoveBreastSegment) {
                        Text(stringResource(R.string.remove_last_segment))
                    }
                }
                val breastNextMinutes = breastMinutes.toLongOrNull()
                val breastTotalMinutes =
                    breastDraftSegments.sumOf { it.second } + (breastNextMinutes ?: 0L)
                OutlinedButton(
                    enabled =
                        breastNextMinutes != null &&
                            breastNextMinutes in 1L..240L &&
                            breastDraftSegments.size < 7 &&
                            breastTotalMinutes <= 240L,
                    onClick = actions.onAddBreastSegment,
                ) {
                    Text(stringResource(R.string.add_breast_segment))
                }
                Button(
                    enabled =
                        breastNextMinutes != null &&
                            breastNextMinutes in 1L..240L &&
                            breastDraftSegments.size < 8 &&
                            breastTotalMinutes <= 240L,
                    onClick = actions.onSaveBreast,
                ) {
                    Text(stringResource(R.string.save_breast))
                }
            }
            if (captureKind == CaptureKind.PUMP) {
                Text(stringResource(R.string.log_pump), style = MaterialTheme.typography.titleLarge)
                OutlinedTextField(
                    value = pumpMinutes,
                    onValueChange = { it -> actions.onPumpMinutesChange(it) },
                    label = { Text(stringResource(R.string.pump_minutes)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    singleLine = true,
                )
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
                    singleLine = true,
                )
                val pumpDuration = pumpMinutes.toLongOrNull() ?: 0L
                val pumpSides = (pumpLeft.toLongOrNull() ?: 0L) + (pumpRight.toLongOrNull() ?: 0L)
                val pumpCanSave =
                    pumpDuration in 1L..240L &&
                        ((pumpTotal.isBlank() && pumpSides > 0L) ||
                            (pumpLeft.isBlank() &&
                                pumpRight.isBlank() &&
                                (pumpTotal.toLongOrNull() ?: 0L) > 0L))
                Button(enabled = pumpCanSave, onClick = actions.onSavePump) {
                    Text(stringResource(R.string.save_pump))
                }
            }
            if (captureKind == CaptureKind.SOLIDS) {
                Text(
                    stringResource(R.string.log_solids),
                    style = MaterialTheme.typography.titleLarge,
                )
                OutlinedTextField(
                    value = solidsFoods,
                    onValueChange = { it -> actions.onSolidsFoodsChange(it) },
                    label = { Text(stringResource(R.string.solids_foods)) },
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(
                    value = solidsAmount,
                    onValueChange = { it -> actions.onSolidsAmountChange(it) },
                    label = { Text(stringResource(R.string.solids_amount)) },
                    modifier = Modifier.fillMaxWidth(),
                )
                Button(enabled = solidsFoods.isNotBlank(), onClick = actions.onSaveSolids) {
                    Text(stringResource(R.string.save_solids))
                }
            }
            if (captureKind == CaptureKind.SLEEP) {
                Text(
                    stringResource(R.string.log_sleep),
                    style = MaterialTheme.typography.titleLarge,
                )
                Text(stringResource(R.string.sleep_place_title))
                listOf(
                        null to R.string.sleep_place_unspecified,
                        1u.toUByte() to R.string.sleep_place_crib,
                        2u.toUByte() to R.string.sleep_place_pram,
                        3u.toUByte() to R.string.sleep_place_contact,
                        4u.toUByte() to R.string.sleep_place_car,
                        5u.toUByte() to R.string.sleep_place_other,
                    )
                    .chunked(2)
                    .forEach { options ->
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            options.forEach { (place, label) ->
                                FilterChip(
                                    selected = sleepPlace == place,
                                    onClick = { actions.onSleepPlaceChange(place) },
                                    label = { Text(stringResource(label)) },
                                )
                            }
                        }
                    }
                Button(onClick = actions.onStartSleep) {
                    Text(stringResource(R.string.start_sleep))
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedTextField(
                        value = sleepMinutes,
                        onValueChange = { it -> actions.onSleepMinutesChange(it) },
                        label = { Text(stringResource(R.string.sleep_minutes)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                        modifier = Modifier.weight(1f),
                        singleLine = true,
                    )
                    Button(
                        enabled = (sleepMinutes.toLongOrNull() ?: 0L) in 1L..1440L,
                        onClick = actions.onSaveSleep,
                    ) {
                        Text(stringResource(R.string.save_sleep))
                    }
                }
            }
            if (captureKind == CaptureKind.GROWTH) {
                Text(
                    stringResource(R.string.log_growth),
                    style = MaterialTheme.typography.titleLarge,
                )
                GrowthUnitChoices(
                    R.string.weight_unit,
                    massUnits,
                    growthWeightUnit,
                    onSelect = { it -> actions.onGrowthWeightUnitChange(it) },
                )
                OutlinedTextField(
                    value = growthWeight,
                    onValueChange = { it -> actions.onGrowthWeightChange(it) },
                    label = { Text(stringResource(R.string.weight)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )
                GrowthUnitChoices(
                    R.string.length_unit,
                    lengthUnits,
                    growthLengthUnit,
                    onSelect = { it -> actions.onGrowthLengthUnitChange(it) },
                )
                OutlinedTextField(
                    value = growthLength,
                    onValueChange = { it -> actions.onGrowthLengthChange(it) },
                    label = { Text(stringResource(R.string.length)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )
                GrowthUnitChoices(
                    R.string.head_unit,
                    lengthUnits,
                    growthHeadUnit,
                    onSelect = { it -> actions.onGrowthHeadUnitChange(it) },
                )
                OutlinedTextField(
                    value = growthHead,
                    onValueChange = { it -> actions.onGrowthHeadChange(it) },
                    label = { Text(stringResource(R.string.head_circumference)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )
                Button(
                    enabled =
                        listOf(growthWeight, growthLength, growthHead).any { it.isNotBlank() } &&
                            validGrowthAmount(growthWeight, growthWeightUnit) &&
                            validGrowthAmount(growthLength, growthLengthUnit) &&
                            validGrowthAmount(growthHead, growthHeadUnit),
                    onClick = actions.onSaveGrowth,
                ) {
                    Text(stringResource(R.string.save_growth))
                }
            }
            if (captureKind == CaptureKind.TEMPERATURE) {
                Text(
                    stringResource(R.string.log_temperature),
                    style = MaterialTheme.typography.titleLarge,
                )
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    listOf(
                            30u.toUByte() to R.string.unit_celsius,
                            31u.toUByte() to R.string.unit_fahrenheit,
                        )
                        .forEach { (unit, label) ->
                            FilterChip(
                                selected = temperatureUnit == unit,
                                onClick = { actions.onTemperatureUnitChange(unit) },
                                label = { Text(stringResource(label)) },
                            )
                        }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
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
                        modifier = Modifier.weight(1f),
                        singleLine = true,
                    )
                    Button(
                        enabled = validTemperature(temperatureEntered),
                        onClick = actions.onSaveTemperature,
                    ) {
                        Text(stringResource(R.string.save_temperature))
                    }
                }
            }
            if (captureKind == CaptureKind.MEDICATION) {
                Text(
                    stringResource(R.string.log_medication),
                    style = MaterialTheme.typography.titleLarge,
                )
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
                Button(
                    enabled =
                        medicationName.isNotBlank() &&
                            doseAmount.isNotBlank() &&
                            doseUnit.isNotBlank(),
                    onClick = actions.onSaveMedication,
                ) {
                    Text(stringResource(R.string.save_medication))
                }
            }
            if (captureKind == CaptureKind.NOTE) {
                Text(stringResource(R.string.log_note), style = MaterialTheme.typography.titleLarge)
                OutlinedTextField(
                    value = noteText,
                    onValueChange = { it -> actions.onNoteTextChange(it) },
                    label = { Text(stringResource(R.string.note_text)) },
                    modifier = Modifier.fillMaxWidth(),
                )
                Button(enabled = noteText.isNotBlank(), onClick = actions.onSaveNote) {
                    Text(stringResource(R.string.save_note))
                }
            }
        }
    }
}
