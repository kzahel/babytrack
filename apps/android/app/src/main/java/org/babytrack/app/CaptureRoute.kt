package org.babytrack.app

import android.app.DatePickerDialog
import android.app.TimePickerDialog
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalContext
import java.time.LocalDateTime
import java.time.ZoneId
import java.util.TimeZone
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.babytrack_core_ffi.ActivityWhen
import uniffi.babytrack_core_ffi.BreastSegmentRow
import uniffi.babytrack_core_ffi.ChildRow
import uniffi.babytrack_core_ffi.FamilyRef
import uniffi.babytrack_core_ffi.MedicationInput
import uniffi.babytrack_core_ffi.PumpInput

internal data class CaptureModel(val state: CaptureUiState, val actions: CaptureActions)

/** Binds the capture draft to its screen state and Rust-backed save actions. */
@Composable
internal fun captureModel(
    draft: CaptureDraftState,
    captureKind: CaptureKind?,
    family: FamilyRef,
    child: ChildRow,
    activeShared: Boolean,
    actions: TrackingActionRouter,
    scope: kotlinx.coroutines.CoroutineScope,
    feedback: TrackerFeedback,
    errorText: String,
    performChange: ((() -> Unit)?, () -> Unit) -> Unit,
    onSelectKind: (CaptureKind) -> Unit,
    finishCapture: () -> Unit,
    requestTimerNotification: () -> Unit,
    lastBottle: Pair<String, UByte>?,
    nowMs: Long,
): CaptureModel {
    val context = LocalContext.current
    return with(draft) {
        fun change(onSaved: (() -> Unit)? = null, action: () -> Unit) =
            performChange(onSaved, action)
        fun logCompleted(onSaved: (() -> Unit)? = null, action: (ActivityWhen) -> Unit) {
            val chosenAt = logAtMs
            val at = logTime()
            change(
                onSaved = {
                    resetLogTime(chosenAt)
                    onSaved?.invoke()
                    finishCapture()
                }
            ) {
                action(at)
            }
        }
        with(feedback) {
            CaptureModel(
                state =
                    CaptureUiState(
                        childName = child.name,
                        captureKind = captureKind,
                        logAtMs = logAtMs,
                        amount = amount,
                        bottleUnit = bottleUnit,
                        bottleContent = bottleContent,
                        breastMinutes = breastMinutes,
                        breastSide = breastSide,
                        breastDraftSegments = breastDraftSegments,
                        pumpMinutes = pumpMinutes,
                        pumpLeft = pumpLeft,
                        pumpRight = pumpRight,
                        pumpTotal = pumpTotal,
                        solidsFoods = solidsFoods,
                        solidsAmount = solidsAmount,
                        sleepMinutes = sleepMinutes,
                        sleepPlace = sleepPlace,
                        growthWeight = growthWeight,
                        growthWeightUnit = growthWeightUnit,
                        growthLength = growthLength,
                        growthLengthUnit = growthLengthUnit,
                        growthHead = growthHead,
                        growthHeadUnit = growthHeadUnit,
                        temperatureEntered = temperatureEntered,
                        temperatureUnit = temperatureUnit,
                        medicationName = medicationName,
                        doseAmount = doseAmount,
                        doseUnit = doseUnit,
                        noteText = noteText,
                        diaperKind = diaperKind,
                        lastBottle = lastBottle,
                        nowMs = nowMs,
                    ),
                actions =
                    CaptureActions(
                        onCaptureKindChange = action@{ kind -> onSelectKind(kind) },
                        onLogTimeChoose = action@{
                                val current = java.util.Calendar.getInstance()
                                DatePickerDialog(
                                        context,
                                        { _, year, month, day ->
                                            TimePickerDialog(
                                                    context,
                                                    { _, hour, minute ->
                                                        val selected =
                                                            LocalDateTime.of(
                                                                    year,
                                                                    month + 1,
                                                                    day,
                                                                    hour,
                                                                    minute,
                                                                )
                                                                .atZone(ZoneId.systemDefault())
                                                                .toInstant()
                                                                .toEpochMilli()
                                                        if (selected > System.currentTimeMillis()) {
                                                            message =
                                                                context.getString(
                                                                    R.string.log_time_future
                                                                )
                                                        } else {
                                                            logAtMs = selected
                                                            message = null
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
                        onResetLogTime = action@{ logAtMs = null },
                        onDiaperKindChange = action@{ kind -> diaperKind = kind },
                        onLogDiaper = action@{ kind ->
                                logCompleted(onSaved = { if (diaperKind == kind) diaperKind = null }) { at ->
                                    actions
                                        .forTarget(activeShared)
                                        .logDiaper(family, child.id, kind, at)
                                }
                            },
                        onBottleContentChange = action@{ content -> bottleContent = content },
                        onRepeatBottle = action@{ entered, unit ->
                                bottleUnit = unit
                                amount = localizedEntered(context, entered)
                            },
                        onBottleUnitChange = action@{ unit ->
                                if (bottleUnit != unit) {
                                    bottleUnit = unit
                                    amount = ""
                                }
                            },
                        onAmountChange = action@{ it ->
                                amount = decimalDraft(it, bottleUnit != 1u.toUByte())
                            },
                        onSaveBottle = action@{
                                val entered = amount
                                val unit = bottleUnit
                                val content = bottleContent
                                logCompleted(onSaved = { if (amount == entered) amount = "" }) { at
                                    ->
                                    actions
                                        .forTarget(activeShared)
                                        .logBottleEntered(
                                            family,
                                            child.id,
                                            canonicalDecimal(entered),
                                            unit,
                                            content,
                                            at,
                                        )
                                }
                            },
                        onBreastSideChange = action@{ side -> breastSide = side },
                        onBreastMinutesChange = action@{ it ->
                                breastMinutes = it.filter(Char::isDigit).take(3)
                            },
                        onRemoveBreastSegment = action@{
                                breastDraftSegments = breastDraftSegments.dropLast(1)
                            },
                        onAddBreastSegment = action@{
                                val minutes = breastMinutes.toLongOrNull() ?: return@action
                                breastDraftSegments = breastDraftSegments + (breastSide to minutes)
                                breastMinutes = ""
                            },
                        onSaveBreast = action@{
                                val minutes = breastMinutes.toLongOrNull() ?: return@action
                                val draft = breastDraftSegments
                                val plan = draft + (breastSide to minutes)
                                val chosenAt = logAtMs
                                val end = logTime()
                                var cursor = end.startUtcMs - plan.sumOf { it.second * 60_000L }
                                val zone = TimeZone.getDefault()
                                val interval =
                                    ActivityWhen(
                                        cursor,
                                        (zone.getOffset(cursor) / 60_000).toShort(),
                                        end.savedAtMs,
                                    )
                                val segments =
                                    plan.map { (side, duration) ->
                                        val next = cursor + duration * 60_000L
                                        BreastSegmentRow(
                                                side,
                                                cursor,
                                                next,
                                                (zone.getOffset(cursor) / 60_000).toShort(),
                                                (zone.getOffset(next) / 60_000).toShort(),
                                            )
                                            .also { cursor = next }
                                    }
                                scope.launch {
                                    runCatching {
                                            withContext(Dispatchers.IO) {
                                                actions
                                                    .forTarget(activeShared)
                                                    .logBreastFeedSegments(
                                                        family,
                                                        child.id,
                                                        segments,
                                                        interval,
                                                    )
                                            }
                                        }
                                        .onSuccess {
                                            if (
                                                breastMinutes.toLongOrNull() == minutes &&
                                                    breastDraftSegments == draft
                                            ) {
                                                breastMinutes = ""
                                                breastDraftSegments = emptyList()
                                            }
                                            version++
                                            message = null
                                            resetLogTime(chosenAt)
                                            finishCapture()
                                        }
                                        .onFailure { message = errorText }
                                }
                            },
                        onPumpMinutesChange = action@{ it ->
                                pumpMinutes = it.filter(Char::isDigit).take(3)
                            },
                        onPumpLeftChange = action@{ it ->
                                pumpLeft = it.filter(Char::isDigit).take(6)
                            },
                        onPumpRightChange = action@{ it ->
                                pumpRight = it.filter(Char::isDigit).take(6)
                            },
                        onPumpTotalChange = action@{ it ->
                                pumpTotal = it.filter(Char::isDigit).take(6)
                            },
                        onSavePump = action@{
                                val enteredMinutes = pumpMinutes
                                val enteredLeft = pumpLeft
                                val enteredRight = pumpRight
                                val enteredTotal = pumpTotal
                                val minutes = pumpMinutes.toLongOrNull() ?: return@action
                                val left = pumpLeft.toLongOrNull()
                                val right = pumpRight.toLongOrNull()
                                val total = pumpTotal.toLongOrNull()
                                val input = PumpInput(left, right, total)
                                val chosenAt = logAtMs
                                val end = logTime()
                                val interval =
                                    ActivityWhen(
                                        end.startUtcMs - minutes * 60_000L,
                                        (TimeZone.getDefault()
                                                .getOffset(end.startUtcMs - minutes * 60_000L) /
                                                60_000)
                                            .toShort(),
                                        end.savedAtMs,
                                    )
                                scope.launch {
                                    runCatching {
                                            withContext(Dispatchers.IO) {
                                                actions
                                                    .forTarget(activeShared)
                                                    .logPump(
                                                        family,
                                                        child.id,
                                                        input,
                                                        interval,
                                                        end.startUtcMs,
                                                    )
                                            }
                                        }
                                        .onSuccess {
                                            if (pumpMinutes == enteredMinutes) pumpMinutes = ""
                                            if (pumpLeft == enteredLeft) pumpLeft = ""
                                            if (pumpRight == enteredRight) pumpRight = ""
                                            if (pumpTotal == enteredTotal) pumpTotal = ""
                                            version++
                                            message = null
                                            resetLogTime(chosenAt)
                                            finishCapture()
                                        }
                                        .onFailure { message = errorText }
                                }
                            },
                        onSolidsFoodsChange = action@{ it -> solidsFoods = it.take(2048) },
                        onSolidsAmountChange = action@{ it -> solidsAmount = it.take(256) },
                        onSaveSolids = action@{
                                val enteredFoods = solidsFoods
                                val enteredAmount = solidsAmount
                                val foods =
                                    solidsFoods.lines().map { it.trim() }.filter { it.isNotEmpty() }
                                val eaten = solidsAmount.trim()
                                val chosenAt = logAtMs
                                val at = logTime()
                                scope.launch {
                                    runCatching {
                                            withContext(Dispatchers.IO) {
                                                actions
                                                    .forTarget(activeShared)
                                                    .logSolids(family, child.id, foods, eaten, at)
                                            }
                                        }
                                        .onSuccess {
                                            if (solidsFoods == enteredFoods) solidsFoods = ""
                                            if (solidsAmount == enteredAmount) solidsAmount = ""
                                            version++
                                            message = null
                                            resetLogTime(chosenAt)
                                            finishCapture()
                                        }
                                        .onFailure { message = errorText }
                                }
                            },
                        onSleepPlaceChange = action@{ place -> sleepPlace = place },
                        onStartSleep = action@{
                                val enteredPlace = sleepPlace
                                change(
                                    onSaved = {
                                        if (sleepPlace == enteredPlace) sleepPlace = null
                                        requestTimerNotification()
                                        finishCapture()
                                    }
                                ) {
                                    actions
                                        .forTarget(activeShared)
                                        .startSleepWithPlace(
                                            family,
                                            child.id,
                                            nowTime(),
                                            enteredPlace,
                                        )
                                }
                            },
                        onSleepMinutesChange = action@{ it ->
                                sleepMinutes = it.filter(Char::isDigit)
                            },
                        onSaveSleep = action@{
                                val enteredMinutes = sleepMinutes
                                val enteredPlace = sleepPlace
                                val duration = sleepMinutes.toLongOrNull() ?: return@action
                                val chosenAt = logAtMs
                                val end = logTime().startUtcMs
                                val start = end - duration * 60_000
                                val zone = TimeZone.getDefault()
                                val whenStarted =
                                    ActivityWhen(
                                        start,
                                        (zone.getOffset(start) / 60_000).toShort(),
                                        System.currentTimeMillis(),
                                    )
                                val endOffset = (zone.getOffset(end) / 60_000).toShort()
                                change(
                                    onSaved = {
                                        resetLogTime(chosenAt)
                                        finishCapture()
                                        if (sleepMinutes == enteredMinutes) sleepMinutes = ""
                                        if (sleepPlace == enteredPlace) sleepPlace = null
                                    }
                                ) {
                                    actions
                                        .forTarget(activeShared)
                                        .logSleepWithPlace(
                                            family,
                                            child.id,
                                            whenStarted,
                                            end,
                                            endOffset,
                                            enteredPlace,
                                        )
                                }
                            },
                        onGrowthWeightUnitChange = action@{ it ->
                                if (growthWeightUnit != it) {
                                    growthWeightUnit = it
                                    growthWeight = ""
                                }
                            },
                        onGrowthWeightChange = action@{ it ->
                                growthWeight = decimalDraft(it, growthWeightUnit != 10u.toUByte())
                            },
                        onGrowthLengthUnitChange = action@{ it ->
                                if (growthLengthUnit != it) {
                                    growthLengthUnit = it
                                    growthLength = ""
                                }
                            },
                        onGrowthLengthChange = action@{ it ->
                                growthLength = decimalDraft(it, growthLengthUnit != 20u.toUByte())
                            },
                        onGrowthHeadUnitChange = action@{ it ->
                                if (growthHeadUnit != it) {
                                    growthHeadUnit = it
                                    growthHead = ""
                                }
                            },
                        onGrowthHeadChange = action@{ it ->
                                growthHead = decimalDraft(it, growthHeadUnit != 20u.toUByte())
                            },
                        onSaveGrowth = action@{
                                val savedWeight = growthWeight
                                val savedLength = growthLength
                                val savedHead = growthHead
                                val input =
                                    growthInput(
                                        savedWeight,
                                        growthWeightUnit,
                                        savedLength,
                                        growthLengthUnit,
                                        savedHead,
                                        growthHeadUnit,
                                    )
                                val chosenAt = logAtMs
                                val at = logTime()
                                scope.launch {
                                    runCatching {
                                            withContext(Dispatchers.IO) {
                                                actions
                                                    .forTarget(activeShared)
                                                    .logGrowthEntered(family, child.id, input, at)
                                            }
                                        }
                                        .onSuccess {
                                            if (growthWeight == savedWeight) growthWeight = ""
                                            if (growthLength == savedLength) growthLength = ""
                                            if (growthHead == savedHead) growthHead = ""
                                            version++
                                            message = null
                                            resetLogTime(chosenAt)
                                            finishCapture()
                                        }
                                        .onFailure { message = errorText }
                                }
                            },
                        onTemperatureUnitChange = action@{ unit ->
                                if (temperatureUnit != unit) {
                                    temperatureUnit = unit
                                    temperatureEntered = ""
                                }
                            },
                        onTemperatureEnteredChange = action@{ it ->
                                temperatureEntered =
                                    decimalDraft(it, fractional = true, signed = true)
                            },
                        onSaveTemperature = action@{
                                val entered = temperatureEntered.trim()
                                val unit = temperatureUnit
                                val chosenAt = logAtMs
                                val at = logTime()
                                scope.launch {
                                    runCatching {
                                            withContext(Dispatchers.IO) {
                                                actions
                                                    .forTarget(activeShared)
                                                    .logTemperatureEntered(
                                                        family,
                                                        child.id,
                                                        canonicalDecimal(entered),
                                                        unit,
                                                        at,
                                                    )
                                            }
                                        }
                                        .onSuccess {
                                            if (temperatureEntered.trim() == entered)
                                                temperatureEntered = ""
                                            version++
                                            message = null
                                            resetLogTime(chosenAt)
                                            finishCapture()
                                        }
                                        .onFailure { message = errorText }
                                }
                            },
                        onMedicationNameChange = action@{ it -> medicationName = it.take(256) },
                        onDoseAmountChange = action@{ it -> doseAmount = it.take(64) },
                        onDoseUnitChange = action@{ it -> doseUnit = it.take(64) },
                        onSaveMedication = action@{
                                val name = medicationName.trim()
                                val amount = doseAmount.trim()
                                val unit = doseUnit.trim()
                                val input = MedicationInput(name, amount, unit)
                                val chosenAt = logAtMs
                                val at = logTime()
                                scope.launch {
                                    runCatching {
                                            withContext(Dispatchers.IO) {
                                                actions
                                                    .forTarget(activeShared)
                                                    .logMedication(family, child.id, input, at)
                                            }
                                        }
                                        .onSuccess {
                                            if (medicationName.trim() == name) medicationName = ""
                                            if (doseAmount.trim() == amount) doseAmount = ""
                                            if (doseUnit.trim() == unit) doseUnit = ""
                                            version++
                                            message = null
                                            resetLogTime(chosenAt)
                                            finishCapture()
                                        }
                                        .onFailure { message = errorText }
                                }
                            },
                        onNoteTextChange = action@{ it -> noteText = it.take(4096) },
                        onSaveNote = action@{
                                val note = noteText.trim()
                                val chosenAt = logAtMs
                                val at = logTime()
                                scope.launch {
                                    runCatching {
                                            withContext(Dispatchers.IO) {
                                                actions
                                                    .forTarget(activeShared)
                                                    .logNote(family, child.id, note, at)
                                            }
                                        }
                                        .onSuccess {
                                            if (noteText.trim() == note) noteText = ""
                                            version++
                                            message = null
                                            resetLogTime(chosenAt)
                                            finishCapture()
                                        }
                                        .onFailure { message = errorText }
                                }
                            },
                    ),
            )
        }
    }
}
