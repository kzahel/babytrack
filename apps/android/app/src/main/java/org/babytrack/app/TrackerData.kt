package org.babytrack.app

import java.time.LocalDate
import java.time.ZoneId
import uniffi.babytrack_core_ffi.ActivityRow
import uniffi.babytrack_core_ffi.ChildRow
import uniffi.babytrack_core_ffi.DaySummaryRow
import uniffi.babytrack_core_ffi.DayWindowRow
import uniffi.babytrack_core_ffi.FamilyRef
import uniffi.babytrack_core_ffi.NativeLocalStore
import uniffi.babytrack_core_ffi.RestoredOriginRow
import uniffi.babytrack_core_ffi.SharedSnapshotRow

internal fun ByteArray.key(): String = joinToString("") { "%02x".format(it) }

internal fun deviceLabelKey(familyId: ByteArray, deviceId: ByteArray): String =
    familyId.key() + ":" + deviceId.key()

internal data class CompletedSave(val atMs: Long, val revision: ULong)

internal enum class TimelineFilter {
    ALL,
    FEEDS,
    SLEEP,
    DIAPERS,
    CARE,
    NOTES;

    fun includes(kind: String): Boolean =
        when (this) {
            ALL -> true
            FEEDS -> kind.startsWith("feed.") || kind == "pump"
            SLEEP -> kind == "sleep"
            DIAPERS -> kind == "diaper"
            CARE -> kind == "growth" || kind == "temperature" || kind == "medication"
            NOTES -> kind == "note"
        }
}

internal val noteEditableKinds =
    setOf(
        "note",
        "feed.breast",
        "feed.bottle",
        "feed.solids",
        "sleep",
        "pump",
        "diaper",
        "growth",
        "medication",
        "temperature",
    )
internal val instantTimeEditableKinds =
    setOf("note", "feed.bottle", "feed.solids", "diaper", "growth", "medication", "temperature")

internal enum class TrackerDestination {
    TODAY,
    HISTORY,
    FAMILY,
    CAPTURE,
}

internal enum class CaptureKind(val label: Int) {
    DIAPER(R.string.event_diaper),
    BOTTLE(R.string.event_bottle),
    BREAST(R.string.event_breast),
    PUMP(R.string.event_pump),
    SOLIDS(R.string.event_solids),
    SLEEP(R.string.event_sleep),
    GROWTH(R.string.event_growth),
    TEMPERATURE(R.string.event_temperature),
    MEDICATION(R.string.event_medication),
    NOTE(R.string.event_note),
}

internal fun activityLabel(kind: String): Int =
    when (kind) {
        "diaper" -> R.string.event_diaper
        "feed.bottle" -> R.string.event_bottle
        "feed.breast" -> R.string.event_breast
        "feed.solids" -> R.string.event_solids
        "sleep" -> R.string.event_sleep
        "pump" -> R.string.event_pump
        "growth" -> R.string.event_growth
        "temperature" -> R.string.event_temperature
        "medication" -> R.string.event_medication
        "note" -> R.string.event_note
        else -> R.string.unknown_activity
    }

internal data class ScreenData(
    val families: List<FamilyRef>,
    val removedFamilies: List<FamilyRef>,
    val familyChildNames: Map<String, String>,
    val activeFamilyKey: String?,
    val activeChildKey: String?,
    val activeFamilyIsLocal: Boolean,
    val children: List<ChildRow>,
    val entries: List<ActivityRow>,
    val revision: ULong,
    val restoredOrigin: RestoredOriginRow?,
    val shared: Boolean,
    val mainSharedSnapshot: SharedSnapshotRow?,
    val recipients: List<FamilyRef>,
    val readyRecipientKeys: Set<String>,
    val joinedSnapshot: SharedSnapshotRow?,
    val unusedInvitationIds: List<ByteArray>?,
    val activeSleepCount: Int,
    val daySummary: DaySummaryRow?,
    val daySummaryDay: LocalDate?,
)

internal fun runningSleepCount(
    store: NativeLocalStore,
    sharing: ShareCoordinator,
    families: List<FamilyRef>,
    recipients: List<FamilyRef>,
): Int {
    val running = HashSet<String>()
    fun add(family: FamilyRef, activities: List<ActivityRow>) {
        for (entry in activities) {
            if (entry.kind == "sleep" && entry.endUtcMs == null) {
                running.add(family.familyId.key() + entry.id.key())
            }
        }
    }
    for (family in families) {
        if (sharing.isShared(family)) {
            runCatching { sharing.snapshot(family).activities }.getOrNull()?.let { add(family, it) }
        } else {
            for (child in store.children(family)) add(family, store.timeline(family, child.id))
        }
    }
    for (family in recipients) {
        runCatching { sharing.snapshot(family).activities }.getOrNull()?.let { add(family, it) }
    }
    return running.size
}

internal fun loadTrackerData(
    store: NativeLocalStore,
    sharing: ShareCoordinator,
    selectedFamily: String?,
    selectedChild: String?,
    selectedRecipient: String?,
): ScreenData {
    val local = store.families()
    val removedLocal = local.filter { sharing.isShared(it) && sharing.isRemoved(it) }
    val activeLocal =
        local.filterNot { candidate ->
            removedLocal.any { it.familyId.contentEquals(candidate.familyId) }
        }
    val allRecipients = sharing.recipientFamilies()
    val removedRecipients = allRecipients.filter(sharing::isRemoved)
    val recipients =
        allRecipients.filterNot { candidate ->
            removedRecipients.any { it.familyId.contentEquals(candidate.familyId) }
        }
    val recipient =
        recipients.find { it.familyId.key() == selectedRecipient } ?: recipients.firstOrNull()
    val readyJoined =
        recipients.mapNotNull { candidate ->
            runCatching { candidate to sharing.snapshot(candidate) }.getOrNull()
        }
    val familyChildNames = mutableMapOf<String, String>()
    for (candidate in activeLocal) {
        val firstChild =
            runCatching {
                    if (sharing.isShared(candidate))
                        sharing.snapshot(candidate).children.firstOrNull()?.name
                    else store.children(candidate).firstOrNull()?.name
                }
                .getOrNull()
        if (firstChild != null) familyChildNames[candidate.familyId.key()] = firstChild
    }
    for ((candidate, snapshot) in readyJoined) {
        snapshot.children.firstOrNull()?.name?.let {
            familyChildNames[candidate.familyId.key()] = it
        }
    }
    val joinedSnapshot =
        readyJoined.find { it.first.familyId.key() == recipient?.familyId?.key() }?.second
    val shown = activeLocal + readyJoined.map { it.first }
    val family = shown.find { it.familyId.key() == selectedFamily } ?: shown.firstOrNull()
    val localFamily =
        family != null && activeLocal.any { it.familyId.key() == family.familyId.key() }
    val recipientSnapshot =
        readyJoined.find { it.first.familyId.key() == family?.familyId?.key() }?.second
    val shared = recipientSnapshot != null || (family?.let(sharing::isShared) ?: false)
    val snapshot =
        recipientSnapshot
            ?: if (shared) sharing.snapshot(family ?: error("Shared Family absent")) else null
    val unusedInvitationIds =
        if (
            family != null &&
                snapshot?.devices?.any {
                    it.deviceId.contentEquals(family.deviceId) && it.role == 2.toUByte()
                } == true
        )
            runCatching { sharing.unusedInvitationIds(family) }.getOrNull()
        else emptyList()
    val kids = snapshot?.children ?: family?.let(store::children).orEmpty()
    val child = kids.find { it.id.key() == selectedChild } ?: kids.firstOrNull()
    val history =
        if (family != null && child != null) {
            snapshot?.activities?.filter { it.childId.contentEquals(child.id) }
                ?: store.timeline(family, child.id)
        } else emptyList()
    val summaryZone = ZoneId.systemDefault()
    val summaryDay = LocalDate.now(summaryZone)
    val window =
        DayWindowRow(
            summaryDay.atStartOfDay(summaryZone).toInstant().toEpochMilli(),
            summaryDay.plusDays(1).atStartOfDay(summaryZone).toInstant().toEpochMilli(),
            System.currentTimeMillis(),
        )
    val daySummary =
        if (family != null && child != null) {
            if (shared) sharing.daySummary(family, child.id, window)
            else store.daySummary(family, child.id, window)
        } else null
    return ScreenData(
        shown,
        (removedLocal + removedRecipients).distinctBy { it.familyId.key() },
        familyChildNames,
        family?.familyId?.key(),
        child?.id?.key(),
        localFamily,
        kids,
        history,
        if (!shared) family?.let(store::revision) ?: 0uL else 0uL,
        if (!shared) family?.let(store::restoredOrigin) else null,
        shared,
        snapshot,
        recipients,
        readyJoined.mapTo(mutableSetOf()) { it.first.familyId.key() },
        joinedSnapshot,
        unusedInvitationIds,
        runningSleepCount(store, sharing, activeLocal, recipients),
        daySummary,
        if (daySummary != null) summaryDay else null,
    )
}
