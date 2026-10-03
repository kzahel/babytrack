package org.babytrack.app

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import java.text.DateFormat
import java.util.Date

internal data class DeleteEntryActions(
    val onDismiss: () -> Unit = {},
    val onConfirmDeleteEntry: () -> Unit = {},
    val onCancel: () -> Unit = {},
)

@Composable
internal fun DeleteEntryDialog(target: PendingActivityDelete, actions: DeleteEntryActions) {
    val context = LocalContext.current
    AlertDialog(
        onDismissRequest = actions.onDismiss,
        title = { Text(stringResource(R.string.delete_entry)) },
        text = { Text(stringResource(R.string.delete_entry_warning)) },
        confirmButton = {
            Button(onClick = actions.onConfirmDeleteEntry) {
                Text(stringResource(R.string.confirm_delete_entry))
            }
        },
        dismissButton = {
            OutlinedButton(onClick = actions.onCancel) { Text(stringResource(R.string.cancel)) }
        },
    )
}

internal data class EditTimeActions(
    val onDismiss: () -> Unit = {},
    val onChooseEntryTime: () -> Unit = {},
    val onSaveChanges: () -> Unit = {},
    val onCancel: () -> Unit = {},
)

@Composable
internal fun EditTimeDialog(target: PendingTimeEdit, actions: EditTimeActions) {
    val context = LocalContext.current
    AlertDialog(
        onDismissRequest = actions.onDismiss,
        title = {
            Text(
                stringResource(
                    if (target.intervalDurationMs == null) R.string.edit_entry_time
                    else R.string.move_completed_session
                )
            )
        },
        text = {
            Column {
                Text(
                    DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT)
                        .format(Date(target.startUtcMs))
                )
                target.intervalDurationMs?.let { duration ->
                    val end = target.movedEndUtcMs ?: target.startUtcMs + duration
                    Text(
                        stringResource(
                            R.string.session_end_time,
                            DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT)
                                .format(Date(end)),
                        )
                    )
                }
                OutlinedButton(onClick = actions.onChooseEntryTime) {
                    Text(stringResource(R.string.choose_entry_time))
                }
            }
        },
        confirmButton = {
            Button(
                enabled =
                    target.intervalDurationMs == null ||
                        target.movedEndUtcMs != null && target.movedEndOffsetMinutes != null,
                onClick = actions.onSaveChanges,
            ) {
                Text(stringResource(R.string.save_changes))
            }
        },
        dismissButton = {
            OutlinedButton(onClick = actions.onCancel) { Text(stringResource(R.string.cancel)) }
        },
    )
}

internal data class EditNoteActions(
    val onDismiss: () -> Unit = {},
    val onTextChange: (String) -> Unit = { _ -> },
    val onSaveChanges: () -> Unit = {},
    val onCancel: () -> Unit = {},
)

@Composable
internal fun EditNoteDialog(target: PendingNoteEdit, actions: EditNoteActions) {
    val context = LocalContext.current
    AlertDialog(
        onDismissRequest = actions.onDismiss,
        title = {
            Text(
                stringResource(
                    if (target.hadNote) R.string.edit_note else R.string.add_activity_note
                )
            )
        },
        text = {
            Column {
                OutlinedTextField(
                    value = target.text,
                    onValueChange = { it -> actions.onTextChange(it) },
                    label = { Text(stringResource(R.string.note_text)) },
                )
                if (!target.standalone && target.hadNote) {
                    Text(stringResource(R.string.activity_note_clear_hint))
                }
            }
        },
        confirmButton = {
            Button(
                enabled = target.text.trim().isNotEmpty() || (!target.standalone && target.hadNote),
                onClick = actions.onSaveChanges,
            ) {
                Text(stringResource(R.string.save_changes))
            }
        },
        dismissButton = {
            OutlinedButton(onClick = actions.onCancel) { Text(stringResource(R.string.cancel)) }
        },
    )
}

internal data class EditBottleActions(
    val onDismiss: () -> Unit = {},
    val onAmountChange: (String) -> Unit = { _ -> },
    val onUnitChange: (UByte) -> Unit = { _ -> },
    val onContentChange: (UByte) -> Unit = { _ -> },
    val onSaveChanges: () -> Unit = {},
    val onCancel: () -> Unit = {},
)

@Composable
internal fun EditBottleDialog(target: PendingBottleEdit, actions: EditBottleActions) {
    val context = LocalContext.current
    AlertDialog(
        onDismissRequest = actions.onDismiss,
        title = { Text(stringResource(R.string.edit_bottle)) },
        text = {
            Column {
                OutlinedTextField(
                    value = target.amount,
                    onValueChange = { it -> actions.onAmountChange(it) },
                    label = { Text(stringResource(R.string.bottle_amount)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                )
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    listOf(
                            1u.toUByte() to R.string.unit_ml,
                            2u.toUByte() to R.string.unit_us_fl_oz,
                            3u.toUByte() to R.string.unit_uk_fl_oz,
                        )
                        .forEach { (unit, label) ->
                            FilterChip(
                                selected = target.unit == unit,
                                onClick = { actions.onUnitChange(unit) },
                                label = { Text(stringResource(label)) },
                            )
                        }
                }
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
                                    selected = target.content == content,
                                    onClick = { actions.onContentChange(content) },
                                    label = { Text(stringResource(label)) },
                                )
                            }
                        }
                    }
            }
        },
        confirmButton = {
            Button(
                enabled = validBottleAmount(target.amount, target.unit),
                onClick = actions.onSaveChanges,
            ) {
                Text(stringResource(R.string.save_changes))
            }
        },
        dismissButton = {
            OutlinedButton(onClick = actions.onCancel) { Text(stringResource(R.string.cancel)) }
        },
    )
}

internal data class EditBreastActions(
    val onDismiss: () -> Unit = {},
    val onChooseStartTime: () -> Unit = {},
    val onKeepFinishTimeChange: (Boolean) -> Unit = {},
    val onSegmentSideChange: (UByte, Int) -> Unit = { _, _ -> },
    val onSegmentMinutesChange: (String, Int) -> Unit = { _, _ -> },
    val onRemoveLastSegment: () -> Unit = {},
    val onAddBreastSegment: () -> Unit = {},
    val onSaveChanges: () -> Unit = {},
    val onCancel: () -> Unit = {},
)

@Composable
internal fun EditBreastDialog(target: PendingBreastEdit, actions: EditBreastActions) {
    val focus = LocalFocusManager.current
    val scroll = rememberScrollState()
    val valid = target.spanMs() != null
    val format = DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT)
    AlertDialog(
        onDismissRequest = { if (!target.saving) actions.onDismiss() },
        title = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(stringResource(R.string.edit_breast))
                target.error?.let {
                    Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.error)
                }
            }
        },
        text = {
            Column(Modifier.verticalScroll(scroll)) {
                OutlinedButton(enabled = valid && !target.saving, onClick = actions.onChooseStartTime) {
                    Text(stringResource(R.string.breast_edit_start, format.format(Date(target.plannedStartMs() ?: target.startUtcMs))))
                }
                target.plannedFinishMs()?.let {
                    Text(stringResource(R.string.breast_edit_finish, format.format(Date(it))))
                }
                Text(stringResource(R.string.breast_edit_anchor))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    listOf(true to R.string.breast_keep_finish, false to R.string.breast_keep_start).forEach { (keepFinish, label) ->
                        FilterChip(
                            selected = target.keepFinishTime == keepFinish,
                            enabled = valid && !target.saving,
                            onClick = { actions.onKeepFinishTimeChange(keepFinish) },
                            label = { Text(stringResource(label)) },
                        )
                    }
                }
                target.segments.forEachIndexed { index, segment ->
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        listOf(
                                1u.toUByte() to R.string.breast_left,
                                2u.toUByte() to R.string.breast_right,
                            )
                            .forEach { (side, label) ->
                                FilterChip(
                                    selected = segment.first == side,
                                    enabled = !target.saving,
                                    onClick = { actions.onSegmentSideChange(side, index) },
                                    label = { Text(stringResource(label)) },
                                )
                            }
                    }
                    OutlinedTextField(
                        value = segment.second,
                        enabled = !target.saving,
                        onValueChange = { it -> actions.onSegmentMinutesChange(it, index) },
                        label = { Text(stringResource(R.string.breast_minutes)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                        singleLine = true,
                    )
                }
                if (target.segments.size > 1) {
                    OutlinedButton(enabled = !target.saving, onClick = actions.onRemoveLastSegment) {
                        Text(stringResource(R.string.remove_last_segment))
                    }
                }
                if (target.segments.size < 8) {
                    OutlinedButton(enabled = !target.saving, onClick = actions.onAddBreastSegment) {
                        Text(stringResource(R.string.add_breast_segment))
                    }
                }
            }
        },
        confirmButton = {
            Button(enabled = valid && !target.saving, onClick = {
                focus.clearFocus()
                actions.onSaveChanges()
            }) {
                Text(stringResource(R.string.save_changes))
            }
        },
        dismissButton = {
            OutlinedButton(enabled = !target.saving, onClick = actions.onCancel) {
                Text(stringResource(R.string.cancel))
            }
        },
    )
}

internal data class EditDiaperActions(
    val onDismiss: () -> Unit = {},
    val onKindChange: (UByte) -> Unit = { _ -> },
    val onSaveChanges: () -> Unit = {},
    val onCancel: () -> Unit = {},
)

@Composable
internal fun EditDiaperDialog(target: PendingDiaperEdit, actions: EditDiaperActions) {
    val context = LocalContext.current
    AlertDialog(
        onDismissRequest = actions.onDismiss,
        title = { Text(stringResource(R.string.edit_diaper)) },
        text = {
            Column {
                listOf(
                        1u.toUByte() to R.string.wet,
                        2u.toUByte() to R.string.dirty,
                        3u.toUByte() to R.string.both,
                        4u.toUByte() to R.string.dry,
                    )
                    .forEach { (kind, label) ->
                        FilterChip(
                            selected = target.kind == kind,
                            onClick = { actions.onKindChange(kind) },
                            label = { Text(stringResource(label)) },
                        )
                    }
            }
        },
        confirmButton = {
            Button(onClick = actions.onSaveChanges) { Text(stringResource(R.string.save_changes)) }
        },
        dismissButton = {
            OutlinedButton(onClick = actions.onCancel) { Text(stringResource(R.string.cancel)) }
        },
    )
}

internal data class EditSolidsActions(
    val onDismiss: () -> Unit = {},
    val onFoodsChange: (String) -> Unit = { _ -> },
    val onAmountChange: (String) -> Unit = { _ -> },
    val onSaveChanges: () -> Unit = {},
    val onCancel: () -> Unit = {},
)

@Composable
internal fun EditSolidsDialog(target: PendingSolidsEdit, actions: EditSolidsActions) {
    val context = LocalContext.current
    AlertDialog(
        onDismissRequest = actions.onDismiss,
        title = { Text(stringResource(R.string.edit_solids)) },
        text = {
            Column {
                OutlinedTextField(
                    value = target.foods,
                    onValueChange = { it -> actions.onFoodsChange(it) },
                    label = { Text(stringResource(R.string.solids_foods)) },
                )
                OutlinedTextField(
                    value = target.amount,
                    onValueChange = { it -> actions.onAmountChange(it) },
                    label = { Text(stringResource(R.string.solids_amount)) },
                )
            }
        },
        confirmButton = {
            Button(enabled = target.foods.isNotBlank(), onClick = actions.onSaveChanges) {
                Text(stringResource(R.string.save_changes))
            }
        },
        dismissButton = {
            OutlinedButton(onClick = actions.onCancel) { Text(stringResource(R.string.cancel)) }
        },
    )
}

internal data class EditGrowthActions(
    val onDismiss: () -> Unit = {},
    val onWeightUnitChange: (UByte) -> Unit = { _ -> },
    val onWeightChange: (String) -> Unit = { _ -> },
    val onLengthUnitChange: (UByte) -> Unit = { _ -> },
    val onLengthChange: (String) -> Unit = { _ -> },
    val onHeadUnitChange: (UByte) -> Unit = { _ -> },
    val onHeadChange: (String) -> Unit = { _ -> },
    val onSaveChanges: () -> Unit = {},
    val onCancel: () -> Unit = {},
)

@Composable
internal fun EditGrowthDialog(target: PendingGrowthEdit, actions: EditGrowthActions) {
    val context = LocalContext.current
    AlertDialog(
        onDismissRequest = actions.onDismiss,
        title = { Text(stringResource(R.string.edit_growth)) },
        text = {
            Column(modifier = Modifier.verticalScroll(rememberScrollState())) {
                GrowthUnitChoices(
                    R.string.weight_unit,
                    massUnits,
                    target.weightUnit,
                    onSelect = { it -> actions.onWeightUnitChange(it) },
                )
                OutlinedTextField(
                    value = target.weight,
                    onValueChange = { it -> actions.onWeightChange(it) },
                    label = { Text(stringResource(R.string.weight)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                    singleLine = true,
                )
                GrowthUnitChoices(
                    R.string.length_unit,
                    lengthUnits,
                    target.lengthUnit,
                    onSelect = { it -> actions.onLengthUnitChange(it) },
                )
                OutlinedTextField(
                    value = target.length,
                    onValueChange = { it -> actions.onLengthChange(it) },
                    label = { Text(stringResource(R.string.length)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                    singleLine = true,
                )
                GrowthUnitChoices(
                    R.string.head_unit,
                    lengthUnits,
                    target.headUnit,
                    onSelect = { it -> actions.onHeadUnitChange(it) },
                )
                OutlinedTextField(
                    value = target.head,
                    onValueChange = { it -> actions.onHeadChange(it) },
                    label = { Text(stringResource(R.string.head_circumference)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                    singleLine = true,
                )
                Text(stringResource(R.string.growth_edit_hint))
            }
        },
        confirmButton = {
            Button(
                enabled =
                    listOf(target.weight, target.length, target.head).any { it.isNotBlank() } &&
                        validGrowthAmount(target.weight, target.weightUnit) &&
                        validGrowthAmount(target.length, target.lengthUnit) &&
                        validGrowthAmount(target.head, target.headUnit),
                onClick = actions.onSaveChanges,
            ) {
                Text(stringResource(R.string.save_changes))
            }
        },
        dismissButton = {
            OutlinedButton(onClick = actions.onCancel) { Text(stringResource(R.string.cancel)) }
        },
    )
}

internal data class EditPumpActions(
    val onDismiss: () -> Unit = {},
    val onLeftChange: (String) -> Unit = { _ -> },
    val onRightChange: (String) -> Unit = { _ -> },
    val onTotalChange: (String) -> Unit = { _ -> },
    val onSaveChanges: () -> Unit = {},
    val onCancel: () -> Unit = {},
)

@Composable
internal fun EditPumpDialog(target: PendingPumpEdit, actions: EditPumpActions) {
    val context = LocalContext.current
    AlertDialog(
        onDismissRequest = actions.onDismiss,
        title = { Text(stringResource(R.string.edit_pump)) },
        text = {
            Column {
                OutlinedTextField(
                    value = target.left,
                    onValueChange = { it -> actions.onLeftChange(it) },
                    label = { Text(stringResource(R.string.pump_left_ml)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    singleLine = true,
                )
                OutlinedTextField(
                    value = target.right,
                    onValueChange = { it -> actions.onRightChange(it) },
                    label = { Text(stringResource(R.string.pump_right_ml)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    singleLine = true,
                )
                OutlinedTextField(
                    value = target.total,
                    onValueChange = { it -> actions.onTotalChange(it) },
                    label = { Text(stringResource(R.string.pump_total_ml)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    singleLine = true,
                )
                Text(stringResource(R.string.pump_edit_hint))
            }
        },
        confirmButton = {
            val left = target.left.toLongOrNull()
            val right = target.right.toLongOrNull()
            val total = target.total.toLongOrNull()
            val valid =
                if (total != null)
                    target.left.isBlank() && target.right.isBlank() && total in 1L..1_000_000L
                else
                    (left ?: 0L) + (right ?: 0L) > 0L &&
                        (left == null || left in 0L..1_000_000L) &&
                        (right == null || right in 0L..1_000_000L)
            Button(enabled = valid, onClick = actions.onSaveChanges) {
                Text(stringResource(R.string.save_changes))
            }
        },
        dismissButton = {
            OutlinedButton(onClick = actions.onCancel) { Text(stringResource(R.string.cancel)) }
        },
    )
}

internal data class EditMedicationActions(
    val onDismiss: () -> Unit = {},
    val onNameChange: (String) -> Unit = { _ -> },
    val onAmountChange: (String) -> Unit = { _ -> },
    val onUnitChange: (String) -> Unit = { _ -> },
    val onSaveChanges: () -> Unit = {},
    val onCancel: () -> Unit = {},
)

@Composable
internal fun EditMedicationDialog(target: PendingMedicationEdit, actions: EditMedicationActions) {
    val context = LocalContext.current
    AlertDialog(
        onDismissRequest = actions.onDismiss,
        title = { Text(stringResource(R.string.edit_medication)) },
        text = {
            Column {
                OutlinedTextField(
                    value = target.name,
                    onValueChange = { it -> actions.onNameChange(it) },
                    label = { Text(stringResource(R.string.medication_name)) },
                    singleLine = true,
                )
                OutlinedTextField(
                    value = target.doseAmount,
                    onValueChange = { it -> actions.onAmountChange(it) },
                    label = { Text(stringResource(R.string.dose_amount)) },
                    singleLine = true,
                )
                OutlinedTextField(
                    value = target.doseUnit,
                    onValueChange = { it -> actions.onUnitChange(it) },
                    label = { Text(stringResource(R.string.dose_unit)) },
                    singleLine = true,
                )
            }
        },
        confirmButton = {
            Button(
                enabled =
                    target.name.isNotBlank() &&
                        target.doseAmount.isNotBlank() &&
                        target.doseUnit.isNotBlank(),
                onClick = actions.onSaveChanges,
            ) {
                Text(stringResource(R.string.save_changes))
            }
        },
        dismissButton = {
            OutlinedButton(onClick = actions.onCancel) { Text(stringResource(R.string.cancel)) }
        },
    )
}

internal data class EditChildActions(
    val onNameChange: (String) -> Unit = { _ -> },
    val onBirthDateChange: (String) -> Unit = { _ -> },
    val onSexChange: (UByte) -> Unit = { _ -> },
    val onDismiss: () -> Unit = {},
    val onSave: () -> Unit = {},
)

@Composable
internal fun EditChildDialog(
    target: PendingChildProfileEdit,
    saving: Boolean,
    actions: EditChildActions,
) {
    val context = LocalContext.current
    ChildProfileScreen(
        editing = true,
        name = target.name,
        birthDate = target.birthDate,
        sex = target.sex,
        saving = saving,
        canClearBirthDate = target.originalBirthDate.isBlank(),
        onNameChange = { it -> actions.onNameChange(it) },
        onBirthDateChange = { it -> actions.onBirthDateChange(it) },
        onSexChange = { it -> actions.onSexChange(it) },
        onDismiss = actions.onDismiss,
        onSave = actions.onSave,
    )
}

internal data class EditSleepActions(
    val onDismiss: () -> Unit = {},
    val onMinutesChange: (String) -> Unit = { _ -> },
    val onSaveChanges: () -> Unit = {},
    val onCancel: () -> Unit = {},
)

@Composable
internal fun EditSleepDialog(target: PendingSleepEdit, actions: EditSleepActions) {
    val context = LocalContext.current
    AlertDialog(
        onDismissRequest = actions.onDismiss,
        title = { Text(stringResource(R.string.edit_sleep)) },
        text = {
            OutlinedTextField(
                value = target.minutes,
                onValueChange = { it -> actions.onMinutesChange(it) },
                label = { Text(stringResource(R.string.sleep_minutes)) },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                singleLine = true,
            )
        },
        confirmButton = {
            val minutes = target.minutes.toLongOrNull()
            val end = minutes?.let { target.startUtcMs + it * 60_000L }
            Button(
                enabled =
                    minutes != null &&
                        minutes in 1L..1440L &&
                        end != null &&
                        end <= System.currentTimeMillis(),
                onClick = actions.onSaveChanges,
            ) {
                Text(stringResource(R.string.save_changes))
            }
        },
        dismissButton = {
            OutlinedButton(onClick = actions.onCancel) { Text(stringResource(R.string.cancel)) }
        },
    )
}

internal data class EditSleepPlaceActions(
    val onDismiss: () -> Unit = {},
    val onPlaceChange: (UByte?) -> Unit = { _ -> },
    val onSaveChanges: () -> Unit = {},
    val onCancel: () -> Unit = {},
)

@Composable
internal fun EditSleepPlaceDialog(target: PendingSleepPlaceEdit, actions: EditSleepPlaceActions) {
    val context = LocalContext.current
    AlertDialog(
        onDismissRequest = actions.onDismiss,
        title = { Text(stringResource(R.string.edit_sleep_place)) },
        text = {
            Column {
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
                                    selected = target.place == place,
                                    onClick = { actions.onPlaceChange(place) },
                                    label = { Text(stringResource(label)) },
                                )
                            }
                        }
                    }
            }
        },
        confirmButton = {
            Button(onClick = actions.onSaveChanges) { Text(stringResource(R.string.save_changes)) }
        },
        dismissButton = {
            OutlinedButton(onClick = actions.onCancel) { Text(stringResource(R.string.cancel)) }
        },
    )
}

internal data class EditTemperatureActions(
    val onDismiss: () -> Unit = {},
    val onAmountChange: (String) -> Unit = { _ -> },
    val onUnitChange: (UByte) -> Unit = { _ -> },
    val onSaveChanges: () -> Unit = {},
    val onCancel: () -> Unit = {},
)

@Composable
internal fun EditTemperatureDialog(
    target: PendingTemperatureEdit,
    actions: EditTemperatureActions,
) {
    val context = LocalContext.current
    AlertDialog(
        onDismissRequest = actions.onDismiss,
        title = { Text(stringResource(R.string.edit_temperature)) },
        text = {
            Column {
                OutlinedTextField(
                    value = target.entered,
                    onValueChange = { it -> actions.onAmountChange(it) },
                    label = {
                        Text(
                            stringResource(
                                if (target.unit == 30u.toUByte()) R.string.temperature_c
                                else R.string.temperature_f
                            )
                        )
                    },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                    singleLine = true,
                )
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    listOf(
                            30u.toUByte() to R.string.unit_celsius,
                            31u.toUByte() to R.string.unit_fahrenheit,
                        )
                        .forEach { (unit, label) ->
                            FilterChip(
                                selected = target.unit == unit,
                                onClick = { actions.onUnitChange(unit) },
                                label = { Text(stringResource(label)) },
                            )
                        }
                }
            }
        },
        confirmButton = {
            Button(enabled = validTemperature(target.entered), onClick = actions.onSaveChanges) {
                Text(stringResource(R.string.save_changes))
            }
        },
        dismissButton = {
            OutlinedButton(onClick = actions.onCancel) { Text(stringResource(R.string.cancel)) }
        },
    )
}
