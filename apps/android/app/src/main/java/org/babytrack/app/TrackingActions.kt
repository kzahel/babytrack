package org.babytrack.app

import uniffi.babytrack_core_ffi.ActivityWhen
import uniffi.babytrack_core_ffi.FamilyRef
import uniffi.babytrack_core_ffi.NativeLocalStore

// Platform dispatch only. Callers retain their saved Family/child/record target.
internal interface TrackingActions {
    fun addChildWithMetadata(
        family: FamilyRef,
        name: String,
        birthDay: Long?,
        sex: UByte?,
        nowMs: Long,
    ): ByteArray

    fun deleteActivity(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        savedAtMs: Long,
    ): Unit

    fun editBottleEntered(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        entered: String,
        unit: UByte,
        content: UByte,
        savedAtMs: Long,
    ): Unit

    fun editBreastFeedSegments(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        segments: List<uniffi.babytrack_core_ffi.BreastSegmentRow>,
        savedAtMs: Long,
    ): Unit

    fun editChildMetadata(
        family: FamilyRef,
        childId: ByteArray,
        birthDay: Long?,
        sex: UByte?,
        savedAtMs: Long,
    ): Unit

    fun editDiaperKind(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        kind: UByte,
        savedAtMs: Long,
    ): Unit

    fun editGrowthEntered(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        input: uniffi.babytrack_core_ffi.GrowthInputRow,
        savedAtMs: Long,
    ): Unit

    fun editInstantTime(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        startUtcMs: Long,
        offsetMinutes: Short,
        savedAtMs: Long,
    ): Unit

    fun editMedication(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        input: uniffi.babytrack_core_ffi.MedicationInput,
        savedAtMs: Long,
    ): Unit

    fun editNote(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        note: String,
        savedAtMs: Long,
    ): Unit

    fun editPumpAmounts(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        input: uniffi.babytrack_core_ffi.PumpInput,
        savedAtMs: Long,
    ): Unit

    fun editSleepEnd(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        endUtcMs: Long,
        endOffsetMinutes: Short,
        savedAtMs: Long,
    ): Unit

    fun editSleepPlace(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        place: UByte?,
        savedAtMs: Long,
    ): Unit

    fun editSolids(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        foods: List<String>,
        amount: String,
        savedAtMs: Long,
    ): Unit

    fun editTemperatureEntered(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        entered: String,
        unit: UByte,
        savedAtMs: Long,
    ): Unit

    fun logBottleEntered(
        family: FamilyRef,
        childId: ByteArray,
        entered: String,
        unit: UByte,
        content: UByte,
        time: ActivityWhen,
    ): ByteArray

    fun logBreastFeedSegments(
        family: FamilyRef,
        childId: ByteArray,
        segments: List<uniffi.babytrack_core_ffi.BreastSegmentRow>,
        time: ActivityWhen,
    ): ByteArray

    fun logDiaper(family: FamilyRef, childId: ByteArray, kind: UByte, time: ActivityWhen): ByteArray

    fun logGrowthEntered(
        family: FamilyRef,
        childId: ByteArray,
        input: uniffi.babytrack_core_ffi.GrowthInputRow,
        time: ActivityWhen,
    ): ByteArray

    fun logMedication(
        family: FamilyRef,
        childId: ByteArray,
        input: uniffi.babytrack_core_ffi.MedicationInput,
        time: ActivityWhen,
    ): ByteArray

    fun logNote(family: FamilyRef, childId: ByteArray, note: String, time: ActivityWhen): ByteArray

    fun logPump(
        family: FamilyRef,
        childId: ByteArray,
        input: uniffi.babytrack_core_ffi.PumpInput,
        time: ActivityWhen,
        endUtcMs: Long,
    ): ByteArray

    fun logSleepWithPlace(
        family: FamilyRef,
        childId: ByteArray,
        time: ActivityWhen,
        endUtcMs: Long,
        endOffsetMinutes: Short,
        place: UByte?,
    ): ByteArray

    fun logSolids(
        family: FamilyRef,
        childId: ByteArray,
        foods: List<String>,
        amount: String,
        time: ActivityWhen,
    ): ByteArray

    fun logTemperatureEntered(
        family: FamilyRef,
        childId: ByteArray,
        entered: String,
        unit: UByte,
        time: ActivityWhen,
    ): ByteArray

    fun moveCompletedInterval(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        time: ActivityWhen,
        endUtcMs: Long,
        endOffsetMinutes: Short,
    ): Unit

    fun renameChild(family: FamilyRef, childId: ByteArray, name: String, savedAtMs: Long): Unit

    fun restoreActivity(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        savedAtMs: Long,
    ): Unit

    fun startSleepWithPlace(
        family: FamilyRef,
        childId: ByteArray,
        time: ActivityWhen,
        place: UByte?,
    ): ByteArray

    fun stopSleep(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        endUtcMs: Long,
        endOffsetMinutes: Short,
    ): Unit
}

internal class TrackingActionRouter(
    private val local: TrackingActions,
    private val shared: TrackingActions,
) {
    fun forTarget(shared: Boolean): TrackingActions = if (shared) this.shared else local
}

internal class NativeTrackingActions(private val store: NativeLocalStore) : TrackingActions {
    override fun addChildWithMetadata(
        family: FamilyRef,
        name: String,
        birthDay: Long?,
        sex: UByte?,
        nowMs: Long,
    ): ByteArray = store.addChildWithMetadata(family, name, birthDay, sex, nowMs)

    override fun deleteActivity(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        savedAtMs: Long,
    ): Unit = store.deleteActivity(family, childId, activityId, savedAtMs)

    override fun editBottleEntered(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        entered: String,
        unit: UByte,
        content: UByte,
        savedAtMs: Long,
    ): Unit =
        store.editBottleEntered(family, childId, activityId, entered, unit, content, savedAtMs)

    override fun editBreastFeedSegments(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        segments: List<uniffi.babytrack_core_ffi.BreastSegmentRow>,
        savedAtMs: Long,
    ): Unit = store.editBreastFeedSegments(family, childId, activityId, segments, savedAtMs)

    override fun editChildMetadata(
        family: FamilyRef,
        childId: ByteArray,
        birthDay: Long?,
        sex: UByte?,
        savedAtMs: Long,
    ): Unit = store.editChildMetadata(family, childId, birthDay, sex, savedAtMs)

    override fun editDiaperKind(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        kind: UByte,
        savedAtMs: Long,
    ): Unit = store.editDiaperKind(family, childId, activityId, kind, savedAtMs)

    override fun editGrowthEntered(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        input: uniffi.babytrack_core_ffi.GrowthInputRow,
        savedAtMs: Long,
    ): Unit = store.editGrowthEntered(family, childId, activityId, input, savedAtMs)

    override fun editInstantTime(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        startUtcMs: Long,
        offsetMinutes: Short,
        savedAtMs: Long,
    ): Unit =
        store.editInstantTime(
            family,
            childId,
            activityId,
            ActivityWhen(startUtcMs, offsetMinutes, savedAtMs),
        )

    override fun editMedication(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        input: uniffi.babytrack_core_ffi.MedicationInput,
        savedAtMs: Long,
    ): Unit = store.editMedication(family, childId, activityId, input, savedAtMs)

    override fun editNote(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        note: String,
        savedAtMs: Long,
    ): Unit = store.editNote(family, childId, activityId, note, savedAtMs)

    override fun editPumpAmounts(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        input: uniffi.babytrack_core_ffi.PumpInput,
        savedAtMs: Long,
    ): Unit = store.editPumpAmounts(family, childId, activityId, input, savedAtMs)

    override fun editSleepEnd(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        endUtcMs: Long,
        endOffsetMinutes: Short,
        savedAtMs: Long,
    ): Unit = store.editSleepEnd(family, childId, activityId, endUtcMs, endOffsetMinutes, savedAtMs)

    override fun editSleepPlace(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        place: UByte?,
        savedAtMs: Long,
    ): Unit = store.editSleepPlace(family, childId, activityId, place, savedAtMs)

    override fun editSolids(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        foods: List<String>,
        amount: String,
        savedAtMs: Long,
    ): Unit = store.editSolids(family, childId, activityId, foods, amount, savedAtMs)

    override fun editTemperatureEntered(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        entered: String,
        unit: UByte,
        savedAtMs: Long,
    ): Unit = store.editTemperatureEntered(family, childId, activityId, entered, unit, savedAtMs)

    override fun logBottleEntered(
        family: FamilyRef,
        childId: ByteArray,
        entered: String,
        unit: UByte,
        content: UByte,
        time: ActivityWhen,
    ): ByteArray = store.logBottleEntered(family, childId, entered, unit, content, time)

    override fun logBreastFeedSegments(
        family: FamilyRef,
        childId: ByteArray,
        segments: List<uniffi.babytrack_core_ffi.BreastSegmentRow>,
        time: ActivityWhen,
    ): ByteArray = store.logBreastFeedSegments(family, childId, segments, time)

    override fun logDiaper(
        family: FamilyRef,
        childId: ByteArray,
        kind: UByte,
        time: ActivityWhen,
    ): ByteArray = store.logDiaper(family, childId, kind, time)

    override fun logGrowthEntered(
        family: FamilyRef,
        childId: ByteArray,
        input: uniffi.babytrack_core_ffi.GrowthInputRow,
        time: ActivityWhen,
    ): ByteArray = store.logGrowthEntered(family, childId, input, time)

    override fun logMedication(
        family: FamilyRef,
        childId: ByteArray,
        input: uniffi.babytrack_core_ffi.MedicationInput,
        time: ActivityWhen,
    ): ByteArray = store.logMedication(family, childId, input, time)

    override fun logNote(
        family: FamilyRef,
        childId: ByteArray,
        note: String,
        time: ActivityWhen,
    ): ByteArray = store.logNote(family, childId, note, time)

    override fun logPump(
        family: FamilyRef,
        childId: ByteArray,
        input: uniffi.babytrack_core_ffi.PumpInput,
        time: ActivityWhen,
        endUtcMs: Long,
    ): ByteArray = store.logPump(family, childId, input, time, endUtcMs)

    override fun logSleepWithPlace(
        family: FamilyRef,
        childId: ByteArray,
        time: ActivityWhen,
        endUtcMs: Long,
        endOffsetMinutes: Short,
        place: UByte?,
    ): ByteArray = store.logSleepWithPlace(family, childId, time, endUtcMs, endOffsetMinutes, place)

    override fun logSolids(
        family: FamilyRef,
        childId: ByteArray,
        foods: List<String>,
        amount: String,
        time: ActivityWhen,
    ): ByteArray = store.logSolids(family, childId, foods, amount, time)

    override fun logTemperatureEntered(
        family: FamilyRef,
        childId: ByteArray,
        entered: String,
        unit: UByte,
        time: ActivityWhen,
    ): ByteArray = store.logTemperatureEntered(family, childId, entered, unit, time)

    override fun moveCompletedInterval(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        time: ActivityWhen,
        endUtcMs: Long,
        endOffsetMinutes: Short,
    ): Unit =
        store.moveCompletedInterval(family, childId, activityId, time, endUtcMs, endOffsetMinutes)

    override fun renameChild(
        family: FamilyRef,
        childId: ByteArray,
        name: String,
        savedAtMs: Long,
    ): Unit = store.renameChild(family, childId, name, savedAtMs)

    override fun restoreActivity(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        savedAtMs: Long,
    ): Unit = store.restoreActivity(family, childId, activityId, savedAtMs)

    override fun startSleepWithPlace(
        family: FamilyRef,
        childId: ByteArray,
        time: ActivityWhen,
        place: UByte?,
    ): ByteArray = store.startSleepWithPlace(family, childId, time, place)

    override fun stopSleep(
        family: FamilyRef,
        childId: ByteArray,
        activityId: ByteArray,
        endUtcMs: Long,
        endOffsetMinutes: Short,
    ): Unit = store.stopSleep(family, childId, activityId, endUtcMs, endOffsetMinutes, endUtcMs)
}
