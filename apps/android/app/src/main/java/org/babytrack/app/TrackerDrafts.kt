package org.babytrack.app

import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import uniffi.babytrack_core_ffi.ActivityWhen

/** Ephemeral presentation state. Durable records and validation remain in Rust. */
@Stable
internal class TrackerFeedback {
    var version by mutableStateOf(0)
    var message by mutableStateOf<String?>(null)
}

@Stable
internal class CaptureDraftState {
    var amount by mutableStateOf("")
    var bottleUnit by mutableStateOf(1u.toUByte())
    var bottleContent by mutableStateOf(2u.toUByte())
    var breastMinutes by mutableStateOf("")
    var breastSide by mutableStateOf(1u.toUByte())
    var breastDraftSegments by mutableStateOf<List<Pair<UByte, Long>>>(emptyList())
    var pumpMinutes by mutableStateOf("")
    var pumpLeft by mutableStateOf("")
    var pumpRight by mutableStateOf("")
    var pumpTotal by mutableStateOf("")
    var solidsFoods by mutableStateOf("")
    var solidsAmount by mutableStateOf("")
    var sleepMinutes by mutableStateOf("")
    var sleepPlace by mutableStateOf<UByte?>(null)
    var noteText by mutableStateOf("")
    var growthWeight by mutableStateOf("")
    var growthWeightUnit by mutableStateOf(11u.toUByte())
    var growthLength by mutableStateOf("")
    var growthLengthUnit by mutableStateOf(21u.toUByte())
    var growthHead by mutableStateOf("")
    var growthHeadUnit by mutableStateOf(21u.toUByte())
    var temperatureEntered by mutableStateOf("")
    var temperatureUnit by mutableStateOf(30u.toUByte())
    var medicationName by mutableStateOf("")
    var doseAmount by mutableStateOf("")
    var doseUnit by mutableStateOf("")
    var logAtMs by mutableStateOf<Long?>(null)

    fun logTime(): ActivityWhen = activityWhen(logAtMs ?: System.currentTimeMillis())

    fun resetLogTime(savedAt: Long?) {
        if (logAtMs == savedAt) logAtMs = null
    }
}

@Stable
internal class EntryEditState {
    var pendingDelete by mutableStateOf<PendingActivityDelete?>(null)
    var recentlyDeleted by mutableStateOf<PendingActivityDelete?>(null)
    var pendingNoteEdit by mutableStateOf<PendingNoteEdit?>(null)
    var pendingTimeEdit by mutableStateOf<PendingTimeEdit?>(null)
    var pendingBottleEdit by mutableStateOf<PendingBottleEdit?>(null)
    var pendingBreastEdit by mutableStateOf<PendingBreastEdit?>(null)
    var pendingDiaperEdit by mutableStateOf<PendingDiaperEdit?>(null)
    var pendingSolidsEdit by mutableStateOf<PendingSolidsEdit?>(null)
    var pendingGrowthEdit by mutableStateOf<PendingGrowthEdit?>(null)
    var pendingPumpEdit by mutableStateOf<PendingPumpEdit?>(null)
    var pendingMedicationEdit by mutableStateOf<PendingMedicationEdit?>(null)
    var pendingChildProfileEdit by mutableStateOf<PendingChildProfileEdit?>(null)
    var pendingSleepEdit by mutableStateOf<PendingSleepEdit?>(null)
    var pendingSleepPlaceEdit by mutableStateOf<PendingSleepPlaceEdit?>(null)
    var pendingTemperatureEdit by mutableStateOf<PendingTemperatureEdit?>(null)
    var childProfileSaving by mutableStateOf(false)
}
