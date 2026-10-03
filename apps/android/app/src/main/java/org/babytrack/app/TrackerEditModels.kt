package org.babytrack.app

import uniffi.babytrack_core_ffi.FamilyRef

internal data class PendingActivityDelete(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
)

internal data class PendingNoteEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val text: String,
    val standalone: Boolean,
    val hadNote: Boolean,
)

internal data class PendingTimeEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val startUtcMs: Long,
    val offsetMinutes: Short,
    val intervalDurationMs: Long? = null,
    val movedEndUtcMs: Long? = null,
    val movedEndOffsetMinutes: Short? = null,
)

internal data class PendingBottleEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val amount: String,
    val unit: UByte,
    val content: UByte,
)

internal data class PendingBreastEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val startUtcMs: Long,
    val startOffsetMinutes: Short,
    val segments: List<Pair<UByte, String>>,
    // Keep pauses from a feed recorded on another client when correcting durations.
    val gapsMs: List<Long>,
    val finishUtcMs: Long,
    val keepFinishTime: Boolean = true,
    val saving: Boolean = false,
    val error: String? = null,
) {
    // Form arithmetic only; the shared core validates and writes the interval.
    fun spanMs(): Long? {
        val minutes = segments.map { it.second.toLongOrNull() }
        if (minutes.any { it == null || it !in 1L..240L } ||
            minutes.filterNotNull().sum() > 240L || segments.isEmpty()) return null
        return minutes.filterNotNull().sum() * 60_000L + gapsMs.sum()
    }

    fun plannedStartMs(): Long? = spanMs()?.let { if (keepFinishTime) finishUtcMs - it else startUtcMs }
    fun plannedFinishMs(): Long? = spanMs()?.let { if (keepFinishTime) finishUtcMs else startUtcMs + it }

    fun keepingFinish(keep: Boolean): PendingBreastEdit = copy(
        startUtcMs = plannedStartMs() ?: startUtcMs,
        startOffsetMinutes = plannedStartMs()?.let {
            if (it == startUtcMs) startOffsetMinutes
            else (java.util.TimeZone.getDefault().getOffset(it) / 60_000).toShort()
        } ?: startOffsetMinutes,
        finishUtcMs = plannedFinishMs() ?: finishUtcMs,
        keepFinishTime = keep,
        error = null,
    )

    fun startingAt(start: Long): PendingBreastEdit = copy(
        startUtcMs = start,
        startOffsetMinutes = (java.util.TimeZone.getDefault().getOffset(start) / 60_000).toShort(),
        finishUtcMs = start + (spanMs() ?: (finishUtcMs - startUtcMs)),
        error = null,
    )
}

internal data class PendingDiaperEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val kind: UByte,
)

internal data class PendingSolidsEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val foods: String,
    val amount: String,
)

internal data class PendingGrowthEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val weight: String,
    val weightUnit: UByte,
    val length: String,
    val lengthUnit: UByte,
    val head: String,
    val headUnit: UByte,
)

internal data class PendingPumpEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val left: String,
    val right: String,
    val total: String,
)

internal data class PendingMedicationEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val name: String,
    val doseAmount: String,
    val doseUnit: String,
)

internal data class PendingChildProfileEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val shared: Boolean,
    val name: String,
    val originalName: String,
    val birthDate: String,
    val originalBirthDate: String,
    val sex: UByte,
    val originalSex: UByte,
)

internal data class PendingSleepEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val startUtcMs: Long,
    val minutes: String,
)

internal data class PendingSleepPlaceEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val place: UByte?,
)

internal data class PendingTemperatureEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val entered: String,
    val unit: UByte,
)

internal data class PendingInvitationCancel(
    val family: FamilyRef,
    val invitationId: ByteArray,
    val localManager: Boolean,
)

internal data class PendingDeviceRemoval(
    val family: FamilyRef,
    val invitationId: ByteArray,
    val deviceId: ByteArray,
    val localManager: Boolean,
)

internal data class PendingRoleChange(
    val family: FamilyRef,
    val targetId: ByteArray,
    val newRole: UByte,
    val localManager: Boolean,
)
