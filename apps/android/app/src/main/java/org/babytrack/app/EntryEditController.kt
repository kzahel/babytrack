package org.babytrack.app

import android.app.DatePickerDialog
import android.app.TimePickerDialog
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalContext
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.ZoneId
import java.util.TimeZone
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.babytrack_core_ffi.ActivityWhen
import uniffi.babytrack_core_ffi.BreastSegmentRow
import uniffi.babytrack_core_ffi.MedicationInput
import uniffi.babytrack_core_ffi.NativeLocalStore
import uniffi.babytrack_core_ffi.PumpInput

@Composable
internal fun EntryEditController(
    edits: EntryEditState,
    store: NativeLocalStore,
    sharing: ShareCoordinator,
    scope: kotlinx.coroutines.CoroutineScope,
    feedback: TrackerFeedback,
    errorText: String,
    performChange: ((() -> Unit)?, () -> Unit) -> Unit,
) {
    val context = LocalContext.current
    fun change(onSaved: (() -> Unit)? = null, action: () -> Unit) = performChange(onSaved, action)
    with(feedback) {
        with(edits) {
            pendingDelete?.let { target ->
                DeleteEntryDialog(
                    target = target,
                    actions =
                        DeleteEntryActions(
                            onDismiss = action@{ pendingDelete = null },
                            onConfirmDeleteEntry = action@{
                                    pendingDelete = null
                                    val savedAtMs = System.currentTimeMillis()
                                    change(onSaved = { recentlyDeleted = target }) {
                                        if (target.shared)
                                            sharing.deleteActivity(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                savedAtMs,
                                            )
                                        else
                                            store.deleteActivity(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                savedAtMs,
                                            )
                                    }
                                },
                            onCancel = action@{ pendingDelete = null },
                        ),
                )
            }
            pendingTimeEdit?.let { target ->
                EditTimeDialog(
                    target = target,
                    actions =
                        EditTimeActions(
                            onDismiss = action@{ pendingTimeEdit = null },
                            onChooseEntryTime = action@{
                                    val current =
                                        java.util.Calendar.getInstance().apply {
                                            timeInMillis = target.startUtcMs
                                        }
                                    DatePickerDialog(
                                            context,
                                            { _, year, month, day ->
                                                TimePickerDialog(
                                                        context,
                                                        { _, hour, minute ->
                                                            if (pendingTimeEdit === target) {
                                                                val selected =
                                                                    LocalDateTime.of(
                                                                            year,
                                                                            month + 1,
                                                                            day,
                                                                            hour,
                                                                            minute,
                                                                        )
                                                                        .atZone(
                                                                            ZoneId.systemDefault()
                                                                        )
                                                                        .toInstant()
                                                                        .toEpochMilli()
                                                                val movedEnd =
                                                                    target.intervalDurationMs
                                                                        ?.let { duration ->
                                                                            runCatching {
                                                                                    Math.addExact(
                                                                                        selected,
                                                                                        duration,
                                                                                    )
                                                                                }
                                                                                .getOrNull()
                                                                        }
                                                                if (
                                                                    selected >
                                                                        System
                                                                            .currentTimeMillis() ||
                                                                        target.intervalDurationMs !=
                                                                            null &&
                                                                            (movedEnd == null ||
                                                                                movedEnd >
                                                                                    System
                                                                                        .currentTimeMillis())
                                                                ) {
                                                                    message =
                                                                        context.getString(
                                                                            R.string.log_time_future
                                                                        )
                                                                } else {
                                                                    pendingTimeEdit =
                                                                        target.copy(
                                                                            startUtcMs = selected,
                                                                            offsetMinutes =
                                                                                (TimeZone
                                                                                        .getDefault()
                                                                                        .getOffset(
                                                                                            selected
                                                                                        ) / 60_000)
                                                                                    .toShort(),
                                                                            movedEndUtcMs =
                                                                                movedEnd,
                                                                            movedEndOffsetMinutes =
                                                                                movedEnd?.let {
                                                                                    (TimeZone
                                                                                            .getDefault()
                                                                                            .getOffset(
                                                                                                it
                                                                                            ) /
                                                                                            60_000)
                                                                                        .toShort()
                                                                                },
                                                                        )
                                                                    message = null
                                                                }
                                                            }
                                                        },
                                                        current.get(java.util.Calendar.HOUR_OF_DAY),
                                                        current.get(java.util.Calendar.MINUTE),
                                                        false,
                                                    )
                                                    .show()
                                            },
                                            current.get(java.util.Calendar.YEAR),
                                            current.get(java.util.Calendar.MONTH),
                                            current.get(java.util.Calendar.DAY_OF_MONTH),
                                        )
                                        .apply { datePicker.maxDate = System.currentTimeMillis() }
                                        .show()
                                },
                            onSaveChanges = action@{
                                    val savedAtMs = System.currentTimeMillis()
                                    change(onSaved = { pendingTimeEdit = null }) {
                                        val end = target.movedEndUtcMs
                                        val endOffset = target.movedEndOffsetMinutes
                                        if (
                                            target.intervalDurationMs != null &&
                                                end != null &&
                                                endOffset != null
                                        ) {
                                            val time =
                                                ActivityWhen(
                                                    target.startUtcMs,
                                                    target.offsetMinutes,
                                                    savedAtMs,
                                                )
                                            if (target.shared)
                                                sharing.moveCompletedInterval(
                                                    target.family,
                                                    target.childId,
                                                    target.activityId,
                                                    time,
                                                    end,
                                                    endOffset,
                                                )
                                            else
                                                store.moveCompletedInterval(
                                                    target.family,
                                                    target.childId,
                                                    target.activityId,
                                                    time,
                                                    end,
                                                    endOffset,
                                                )
                                        } else if (target.shared)
                                            sharing.editInstantTime(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                target.startUtcMs,
                                                target.offsetMinutes,
                                                savedAtMs,
                                            )
                                        else
                                            store.editInstantTime(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                ActivityWhen(
                                                    target.startUtcMs,
                                                    target.offsetMinutes,
                                                    savedAtMs,
                                                ),
                                            )
                                    }
                                },
                            onCancel = action@{ pendingTimeEdit = null },
                        ),
                )
            }
            pendingNoteEdit?.let { target ->
                EditNoteDialog(
                    target = target,
                    actions =
                        EditNoteActions(
                            onDismiss = action@{ pendingNoteEdit = null },
                            onTextChange = action@{ it ->
                                    pendingNoteEdit = target.copy(text = it.take(4096))
                                },
                            onSaveChanges = action@{
                                    val savedAtMs = System.currentTimeMillis()
                                    change(onSaved = { pendingNoteEdit = null }) {
                                        if (target.shared)
                                            sharing.editNote(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                target.text,
                                                savedAtMs,
                                            )
                                        else
                                            store.editNote(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                target.text,
                                                savedAtMs,
                                            )
                                    }
                                },
                            onCancel = action@{ pendingNoteEdit = null },
                        ),
                )
            }
            pendingBottleEdit?.let { target ->
                EditBottleDialog(
                    target = target,
                    actions =
                        EditBottleActions(
                            onDismiss = action@{ pendingBottleEdit = null },
                            onAmountChange = action@{ it ->
                                    pendingBottleEdit =
                                        target.copy(
                                            amount = decimalDraft(it, target.unit != 1u.toUByte())
                                        )
                                },
                            onUnitChange = action@{ unit ->
                                    if (target.unit != unit)
                                        pendingBottleEdit = target.copy(amount = "", unit = unit)
                                },
                            onContentChange = action@{ content ->
                                    pendingBottleEdit = target.copy(content = content)
                                },
                            onSaveChanges = action@{
                                    val savedAtMs = System.currentTimeMillis()
                                    change(onSaved = { pendingBottleEdit = null }) {
                                        if (target.shared)
                                            sharing.editBottleEntered(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                canonicalDecimal(target.amount),
                                                target.unit,
                                                target.content,
                                                savedAtMs,
                                            )
                                        else
                                            store.editBottleEntered(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                canonicalDecimal(target.amount),
                                                target.unit,
                                                target.content,
                                                savedAtMs,
                                            )
                                    }
                                },
                            onCancel = action@{ pendingBottleEdit = null },
                        ),
                )
            }
            pendingBreastEdit?.let { target ->
                EditBreastDialog(
                    target = target,
                    actions =
                        EditBreastActions(
                            onDismiss = action@{ pendingBreastEdit = null },
                            onSegmentSideChange = action@{ side, index ->
                                    pendingBreastEdit =
                                        target.copy(
                                            segments =
                                                target.segments.mapIndexed { i, value ->
                                                    if (i == index) side to value.second else value
                                                }
                                        )
                                },
                            onSegmentMinutesChange = action@{ minutes, index ->
                                    pendingBreastEdit =
                                        target.copy(
                                            segments =
                                                target.segments.mapIndexed { i, value ->
                                                    if (i == index)
                                                        value.first to
                                                            minutes.filter(Char::isDigit).take(3)
                                                    else value
                                                }
                                        )
                                },
                            onRemoveLastSegment = action@{
                                    pendingBreastEdit =
                                        target.copy(
                                            segments = target.segments.dropLast(1),
                                            gapsMs = target.gapsMs.dropLast(1),
                                        )
                                },
                            onAddBreastSegment = action@{
                                    val nextSide =
                                        if (target.segments.last().first == 1u.toUByte())
                                            2u.toUByte()
                                        else 1u.toUByte()
                                    pendingBreastEdit =
                                        target.copy(
                                            segments = target.segments + (nextSide to "5"),
                                            gapsMs = target.gapsMs + 0L,
                                        )
                                },
                            onSaveChanges = action@{
                                    val durations = target.segments.map { it.second.toLongOrNull() }
                                    val savedAtMs = System.currentTimeMillis()
                                    var cursor = target.startUtcMs
                                    val zone = TimeZone.getDefault()
                                    val segments =
                                        target.segments.mapIndexed { index, (side, minutes) ->
                                            cursor += target.gapsMs[index]
                                            val next =
                                                cursor +
                                                    (durations[index]
                                                        ?: error("Missing duration")) * 60_000L
                                            BreastSegmentRow(
                                                    side,
                                                    cursor,
                                                    next,
                                                    if (index == 0) target.startOffsetMinutes
                                                    else
                                                        (zone.getOffset(cursor) / 60_000).toShort(),
                                                    (zone.getOffset(next) / 60_000).toShort(),
                                                )
                                                .also { cursor = next }
                                        }
                                    pendingBreastEdit = null
                                    change {
                                        if (target.shared)
                                            sharing.editBreastFeedSegments(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                segments,
                                                savedAtMs,
                                            )
                                        else
                                            store.editBreastFeedSegments(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                segments,
                                                savedAtMs,
                                            )
                                    }
                                },
                            onCancel = action@{ pendingBreastEdit = null },
                        ),
                )
            }
            pendingDiaperEdit?.let { target ->
                EditDiaperDialog(
                    target = target,
                    actions =
                        EditDiaperActions(
                            onDismiss = action@{ pendingDiaperEdit = null },
                            onKindChange = action@{ kind ->
                                    pendingDiaperEdit = target.copy(kind = kind)
                                },
                            onSaveChanges = action@{
                                    pendingDiaperEdit = null
                                    val savedAtMs = System.currentTimeMillis()
                                    change {
                                        if (target.shared)
                                            sharing.editDiaperKind(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                target.kind,
                                                savedAtMs,
                                            )
                                        else
                                            store.editDiaperKind(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                target.kind,
                                                savedAtMs,
                                            )
                                    }
                                },
                            onCancel = action@{ pendingDiaperEdit = null },
                        ),
                )
            }
            pendingSolidsEdit?.let { target ->
                EditSolidsDialog(
                    target = target,
                    actions =
                        EditSolidsActions(
                            onDismiss = action@{ pendingSolidsEdit = null },
                            onFoodsChange = action@{ it ->
                                    pendingSolidsEdit = target.copy(foods = it.take(2048))
                                },
                            onAmountChange = action@{ it ->
                                    pendingSolidsEdit = target.copy(amount = it.take(256))
                                },
                            onSaveChanges = action@{
                                    pendingSolidsEdit = null
                                    val savedAtMs = System.currentTimeMillis()
                                    val foods =
                                        target.foods
                                            .lines()
                                            .map { it.trim() }
                                            .filter { it.isNotEmpty() }
                                    change {
                                        if (target.shared)
                                            sharing.editSolids(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                foods,
                                                target.amount,
                                                savedAtMs,
                                            )
                                        else
                                            store.editSolids(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                foods,
                                                target.amount,
                                                savedAtMs,
                                            )
                                    }
                                },
                            onCancel = action@{ pendingSolidsEdit = null },
                        ),
                )
            }
            pendingGrowthEdit?.let { target ->
                EditGrowthDialog(
                    target = target,
                    actions =
                        EditGrowthActions(
                            onDismiss = action@{ pendingGrowthEdit = null },
                            onWeightUnitChange = action@{ it ->
                                    if (target.weightUnit != it)
                                        pendingGrowthEdit =
                                            target.copy(weight = "", weightUnit = it)
                                },
                            onWeightChange = action@{ it ->
                                    pendingGrowthEdit =
                                        target.copy(
                                            weight =
                                                decimalDraft(it, target.weightUnit != 10u.toUByte())
                                        )
                                },
                            onLengthUnitChange = action@{ it ->
                                    if (target.lengthUnit != it)
                                        pendingGrowthEdit =
                                            target.copy(length = "", lengthUnit = it)
                                },
                            onLengthChange = action@{ it ->
                                    pendingGrowthEdit =
                                        target.copy(
                                            length =
                                                decimalDraft(it, target.lengthUnit != 20u.toUByte())
                                        )
                                },
                            onHeadUnitChange = action@{ it ->
                                    if (target.headUnit != it)
                                        pendingGrowthEdit = target.copy(head = "", headUnit = it)
                                },
                            onHeadChange = action@{ it ->
                                    pendingGrowthEdit =
                                        target.copy(
                                            head =
                                                decimalDraft(it, target.headUnit != 20u.toUByte())
                                        )
                                },
                            onSaveChanges = action@{
                                    val input =
                                        growthInput(
                                            target.weight,
                                            target.weightUnit,
                                            target.length,
                                            target.lengthUnit,
                                            target.head,
                                            target.headUnit,
                                        )
                                    val savedAtMs = System.currentTimeMillis()
                                    change(onSaved = { pendingGrowthEdit = null }) {
                                        if (target.shared)
                                            sharing.editGrowthEntered(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                input,
                                                savedAtMs,
                                            )
                                        else
                                            store.editGrowthEntered(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                input,
                                                savedAtMs,
                                            )
                                    }
                                },
                            onCancel = action@{ pendingGrowthEdit = null },
                        ),
                )
            }
            pendingPumpEdit?.let { target ->
                EditPumpDialog(
                    target = target,
                    actions =
                        EditPumpActions(
                            onDismiss = action@{ pendingPumpEdit = null },
                            onLeftChange = action@{ it ->
                                    pendingPumpEdit =
                                        target.copy(left = it.filter(Char::isDigit).take(6))
                                },
                            onRightChange = action@{ it ->
                                    pendingPumpEdit =
                                        target.copy(right = it.filter(Char::isDigit).take(6))
                                },
                            onTotalChange = action@{ it ->
                                    pendingPumpEdit =
                                        target.copy(total = it.filter(Char::isDigit).take(6))
                                },
                            onSaveChanges = action@{
                                    pendingPumpEdit = null
                                    val input =
                                        PumpInput(
                                            target.left.toLongOrNull(),
                                            target.right.toLongOrNull(),
                                            target.total.toLongOrNull(),
                                        )
                                    val savedAtMs = System.currentTimeMillis()
                                    change {
                                        if (target.shared)
                                            sharing.editPumpAmounts(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                input,
                                                savedAtMs,
                                            )
                                        else
                                            store.editPumpAmounts(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                input,
                                                savedAtMs,
                                            )
                                    }
                                },
                            onCancel = action@{ pendingPumpEdit = null },
                        ),
                )
            }
            pendingMedicationEdit?.let { target ->
                EditMedicationDialog(
                    target = target,
                    actions =
                        EditMedicationActions(
                            onDismiss = action@{ pendingMedicationEdit = null },
                            onNameChange = action@{ it ->
                                    pendingMedicationEdit = target.copy(name = it.take(256))
                                },
                            onAmountChange = action@{ it ->
                                    pendingMedicationEdit = target.copy(doseAmount = it.take(64))
                                },
                            onUnitChange = action@{ it ->
                                    pendingMedicationEdit = target.copy(doseUnit = it.take(64))
                                },
                            onSaveChanges = action@{
                                    pendingMedicationEdit = null
                                    val input =
                                        MedicationInput(
                                            target.name.trim(),
                                            target.doseAmount.trim(),
                                            target.doseUnit.trim(),
                                        )
                                    val savedAtMs = System.currentTimeMillis()
                                    change {
                                        if (target.shared)
                                            sharing.editMedication(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                input,
                                                savedAtMs,
                                            )
                                        else
                                            store.editMedication(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                input,
                                                savedAtMs,
                                            )
                                    }
                                },
                            onCancel = action@{ pendingMedicationEdit = null },
                        ),
                )
            }
            pendingChildProfileEdit?.let { target ->
                EditChildDialog(
                    target = target,
                    saving = childProfileSaving,
                    actions =
                        EditChildActions(
                            onNameChange = action@{ it ->
                                    pendingChildProfileEdit = target.copy(name = it)
                                },
                            onBirthDateChange = action@{ it ->
                                    pendingChildProfileEdit = target.copy(birthDate = it)
                                },
                            onSexChange = action@{ it ->
                                    pendingChildProfileEdit = target.copy(sex = it)
                                },
                            onDismiss = action@{
                                    if (!childProfileSaving) pendingChildProfileEdit = null
                                },
                            onSave = action@{
                                    val name = target.name.trim()
                                    if (name.isEmpty()) return@action
                                    val birthDay =
                                        runCatching {
                                                target.birthDate
                                                    .takeIf { it.isNotBlank() }
                                                    ?.let { LocalDate.parse(it).toEpochDay() }
                                            }
                                            .getOrElse {
                                                message =
                                                    context.getString(R.string.birth_date_invalid)
                                                return@action
                                            }
                                    if (
                                        name == target.originalName &&
                                            target.birthDate == target.originalBirthDate &&
                                            target.sex == target.originalSex
                                    ) {
                                        pendingChildProfileEdit = null
                                        return@action
                                    }
                                    childProfileSaving = true
                                    scope.launch {
                                        runCatching {
                                                withContext(Dispatchers.IO) {
                                                    if (name != target.originalName) {
                                                        if (target.shared)
                                                            sharing.renameChild(
                                                                target.family,
                                                                target.childId,
                                                                name,
                                                                System.currentTimeMillis(),
                                                            )
                                                        else
                                                            store.renameChild(
                                                                target.family,
                                                                target.childId,
                                                                name,
                                                                System.currentTimeMillis(),
                                                            )
                                                    }
                                                    if (
                                                        target.birthDate !=
                                                            target.originalBirthDate ||
                                                            target.sex != target.originalSex
                                                    ) {
                                                        if (target.shared)
                                                            sharing.editChildMetadata(
                                                                target.family,
                                                                target.childId,
                                                                birthDay,
                                                                target.sex,
                                                                System.currentTimeMillis(),
                                                            )
                                                        else
                                                            store.editChildMetadata(
                                                                target.family,
                                                                target.childId,
                                                                birthDay,
                                                                target.sex,
                                                                System.currentTimeMillis(),
                                                            )
                                                    }
                                                }
                                            }
                                            .onSuccess {
                                                pendingChildProfileEdit = null
                                                version++
                                                message = null
                                            }
                                            .onFailure {
                                                version++
                                                message = errorText
                                            }
                                        childProfileSaving = false
                                    }
                                },
                        ),
                )
            }
            pendingSleepEdit?.let { target ->
                EditSleepDialog(
                    target = target,
                    actions =
                        EditSleepActions(
                            onDismiss = action@{ pendingSleepEdit = null },
                            onMinutesChange = action@{ it ->
                                    pendingSleepEdit =
                                        target.copy(minutes = it.filter(Char::isDigit).take(4))
                                },
                            onSaveChanges = action@{
                                    pendingSleepEdit = null
                                    val savedAtMs = System.currentTimeMillis()
                                    val newEnd =
                                        target.minutes.toLongOrNull()?.let {
                                            target.startUtcMs + it * 60_000L
                                        } ?: error("Sleep end missing")
                                    val offset =
                                        (TimeZone.getDefault().getOffset(newEnd) / 60_000).toShort()
                                    change {
                                        if (target.shared)
                                            sharing.editSleepEnd(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                newEnd,
                                                offset,
                                                savedAtMs,
                                            )
                                        else
                                            store.editSleepEnd(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                newEnd,
                                                offset,
                                                savedAtMs,
                                            )
                                    }
                                },
                            onCancel = action@{ pendingSleepEdit = null },
                        ),
                )
            }
            pendingSleepPlaceEdit?.let { target ->
                EditSleepPlaceDialog(
                    target = target,
                    actions =
                        EditSleepPlaceActions(
                            onDismiss = action@{ pendingSleepPlaceEdit = null },
                            onPlaceChange = action@{ place ->
                                    pendingSleepPlaceEdit = target.copy(place = place)
                                },
                            onSaveChanges = action@{
                                    pendingSleepPlaceEdit = null
                                    val savedAtMs = System.currentTimeMillis()
                                    change {
                                        if (target.shared)
                                            sharing.editSleepPlace(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                target.place,
                                                savedAtMs,
                                            )
                                        else
                                            store.editSleepPlace(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                target.place,
                                                savedAtMs,
                                            )
                                    }
                                },
                            onCancel = action@{ pendingSleepPlaceEdit = null },
                        ),
                )
            }
            pendingTemperatureEdit?.let { target ->
                EditTemperatureDialog(
                    target = target,
                    actions =
                        EditTemperatureActions(
                            onDismiss = action@{ pendingTemperatureEdit = null },
                            onAmountChange = action@{ it ->
                                    pendingTemperatureEdit =
                                        target.copy(
                                            entered =
                                                decimalDraft(it, fractional = true, signed = true)
                                        )
                                },
                            onUnitChange = action@{ unit ->
                                    if (target.unit != unit)
                                        pendingTemperatureEdit =
                                            target.copy(entered = "", unit = unit)
                                },
                            onSaveChanges = action@{
                                    val savedAtMs = System.currentTimeMillis()
                                    change(onSaved = { pendingTemperatureEdit = null }) {
                                        if (target.shared)
                                            sharing.editTemperatureEntered(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                canonicalDecimal(target.entered),
                                                target.unit,
                                                savedAtMs,
                                            )
                                        else
                                            store.editTemperatureEntered(
                                                target.family,
                                                target.childId,
                                                target.activityId,
                                                canonicalDecimal(target.entered),
                                                target.unit,
                                                savedAtMs,
                                            )
                                    }
                                },
                            onCancel = action@{ pendingTemperatureEdit = null },
                        ),
                )
            }
        }
    }
}
