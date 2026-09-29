package org.babytrack.app

import android.Manifest
import android.os.Bundle
import android.os.Build
import android.app.ActivityManager
import android.app.DatePickerDialog
import android.app.TimePickerDialog
import android.content.Context
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.content.pm.PackageManager
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.unit.dp
import androidx.compose.runtime.saveable.rememberSaveable
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import androidx.compose.runtime.rememberCoroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import uniffi.babytrack_core_ffi.ActivityRow
import uniffi.babytrack_core_ffi.ActivityWhen
import uniffi.babytrack_core_ffi.BackupFileRow
import uniffi.babytrack_core_ffi.BackupInfoRow
import uniffi.babytrack_core_ffi.BreastSegmentRow
import uniffi.babytrack_core_ffi.ChildRow
import uniffi.babytrack_core_ffi.DaySummaryRow
import uniffi.babytrack_core_ffi.DayWindowRow
import uniffi.babytrack_core_ffi.FamilyRef
import uniffi.babytrack_core_ffi.EnteredMeasureRow
import uniffi.babytrack_core_ffi.GrowthInputRow
import uniffi.babytrack_core_ffi.NativeLocalStore
import uniffi.babytrack_core_ffi.MedicationInput
import uniffi.babytrack_core_ffi.PumpInput
import uniffi.babytrack_core_ffi.RemovedDeviceRow
import uniffi.babytrack_core_ffi.RestoredOriginRow
import uniffi.babytrack_core_ffi.SharedSnapshotRow
import uniffi.babytrack_core_ffi.SharedSyncRow
import java.text.DateFormat
import java.text.DecimalFormatSymbols
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.Instant
import java.time.ZoneId
import java.util.Date
import java.util.TimeZone

class MainActivity : ComponentActivity() {
    private var incomingInvitation by mutableStateOf<String?>(null)
    private var invitationConsumed = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        invitationConsumed = savedInstanceState?.getBoolean("invitation_consumed") ?: false
        incomingInvitation = if (invitationConsumed) null else invitationFrom(intent)
        runCatching { SharedSyncJobService.schedule(this) }
            .onFailure { Log.w("BabytrackSync", "Could not schedule periodic shared sync", it) }
        val app = application as BabytrackApplication
        val savedFiles = getSharedPreferences("completed_file_saves", MODE_PRIVATE)
        val relayOrigins = getSharedPreferences("shared_relay_origins", MODE_PRIVATE)
        val availableMemory = {
            ActivityManager.MemoryInfo().also {
                (getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager).getMemoryInfo(it)
            }.availMem
        }
        setContent {
            BabytrackTheme {
                TrackerScreen(
                    store = app.localStore,
                    sharing = app.sharing,
                    incomingInvitation = incomingInvitation,
                    onInvitationConsumed = {
                        invitationConsumed = true
                        incomingInvitation = null
                    },
                    readFile = { uri -> contentResolver.openInputStream(uri)?.use {
                        readBounded(it, backupReadLimit(availableMemory()))
                    } },
                    writeFile = { uri, bytes ->
                        contentResolver.openOutputStream(uri)?.use { it.write(bytes) } ?: error("No output stream")
                    },
                    availableMemory = availableMemory,
                    lastSave = { family ->
                        savedFiles.getString(family.familyId.key(), null)?.split(':')?.let { parts ->
                            if (parts.size == 2) {
                                val at = parts[0].toLongOrNull()
                                val revision = parts[1].toULongOrNull()
                                if (at != null && revision != null) CompletedSave(at, revision) else null
                            } else null
                        }
                    },
                    recordSave = { file ->
                        savedFiles.edit().putString(
                            file.info.sourceFamilyId.key(),
                            "${file.info.snapshotUtcMs}:${file.revision}",
                        ).commit()
                    },
                    lastRelayOrigin = { family -> relayOrigins.getString(family.familyId.key(), null) },
                    recordRelayOrigin = { family, origin ->
                        relayOrigins.edit().putString(family.familyId.key(), origin).commit()
                    },
                )
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        invitationConsumed = false
        incomingInvitation = invitationFrom(intent)
    }

    override fun onSaveInstanceState(outState: Bundle) {
        outState.putBoolean("invitation_consumed", invitationConsumed)
        super.onSaveInstanceState(outState)
    }
}

private fun invitationFrom(intent: Intent?): String? {
    val text = when (intent?.action) {
        Intent.ACTION_SEND -> if (intent.type == "text/plain") {
            intent.getStringExtra(Intent.EXTRA_TEXT)?.trim()
        } else null
        Intent.ACTION_VIEW -> intent.data?.toString()
        else -> null
    } ?: return null
    val fragment = if (text.startsWith("#")) text else {
        val uri = runCatching { android.net.Uri.parse(text) }.getOrNull() ?: return null
        if (uri.scheme != "babytrack" || uri.host != "join" ||
            !uri.path.isNullOrEmpty() || uri.port != -1 || uri.userInfo != null ||
            uri.query != null) return null
        "#${uri.encodedFragment ?: return null}"
    }
    return fragment.takeIf { it.length <= 2048 && it.startsWith("#bt-invite=v1.") }
}

private fun invitationLink(fragment: String): String = "babytrack://join$fragment"

private fun browserInvitationLink(origin: String, fragment: String): String = "$origin/$fragment"

private fun shareInvitation(context: Context, link: String) {
    val send = Intent(Intent.ACTION_SEND).apply {
        type = "text/plain"
        putExtra(Intent.EXTRA_TEXT, link)
    }
    context.startActivity(Intent.createChooser(send, context.getString(R.string.share_invitation)))
}

private fun copyInvitation(context: Context, link: String) {
    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    clipboard.setPrimaryClip(ClipData.newPlainText(context.getString(R.string.share_invitation), link))
}

private fun ByteArray.key(): String = joinToString("") { "%02x".format(it) }

private fun deviceLabelKey(familyId: ByteArray, deviceId: ByteArray): String =
    familyId.key() + ":" + deviceId.key()

private data class CompletedSave(val atMs: Long, val revision: ULong)
private data class RemovalCopyNotice(val sourceKey: String, val copy: FamilyRef)
private enum class TimelineFilter {
    ALL, FEEDS, SLEEP, DIAPERS, CARE, NOTES;

    fun includes(kind: String): Boolean = when (this) {
        ALL -> true
        FEEDS -> kind.startsWith("feed.") || kind == "pump"
        SLEEP -> kind == "sleep"
        DIAPERS -> kind == "diaper"
        CARE -> kind == "growth" || kind == "temperature" || kind == "medication"
        NOTES -> kind == "note"
    }
}

private data class PendingActivityDelete(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
)
private data class PendingNoteEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val text: String,
    val standalone: Boolean,
    val hadNote: Boolean,
)

private val noteEditableKinds = setOf(
    "note", "feed.breast", "feed.bottle", "feed.solids", "sleep", "pump",
    "diaper", "growth", "medication", "temperature",
)
private val instantTimeEditableKinds = setOf(
    "note", "feed.bottle", "feed.solids", "diaper", "growth", "medication", "temperature",
)
private enum class TrackerDestination { TODAY, HISTORY, FAMILY, CAPTURE }
private enum class CaptureKind(val label: Int) {
    DIAPER(R.string.log_diaper),
    BOTTLE(R.string.log_bottle),
    BREAST(R.string.log_breast),
    PUMP(R.string.log_pump),
    SOLIDS(R.string.log_solids),
    SLEEP(R.string.log_sleep),
    GROWTH(R.string.log_growth),
    TEMPERATURE(R.string.log_temperature),
    MEDICATION(R.string.log_medication),
    NOTE(R.string.log_note),
}
private fun activityLabel(kind: String): Int = when (kind) {
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
private data class PendingTimeEdit(
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
private data class PendingBottleEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val amount: String,
    val unit: UByte,
    val content: UByte,
)

private val bottleAmountPattern = Regex("(0|[1-9][0-9]*)(\\.[0-9]+)?")
private val temperaturePattern = Regex("-?(0|[1-9][0-9]*)(\\.[0-9]+)?")

private fun validBottleAmount(value: String, unit: UByte): Boolean =
    value.length <= 16 && bottleAmountPattern.matches(canonicalDecimal(value)) &&
        (unit != 1u.toUByte() || !canonicalDecimal(value).contains('.')) &&
        value.any { it in '1'..'9' } &&
        (unit != 1u.toUByte() || canonicalDecimal(value).toLongOrNull()?.let { it in 1..1_000_000 } == true)

private fun validGrowthAmount(value: String, unit: UByte): Boolean =
    value.isBlank() || (value.length <= 16 && bottleAmountPattern.matches(canonicalDecimal(value)) &&
        value.any { it in '1'..'9' } &&
        (unit !in listOf(10u.toUByte(), 20u.toUByte()) || !canonicalDecimal(value).contains('.')))

private fun validTemperature(value: String): Boolean =
    value.length <= 16 && temperaturePattern.matches(canonicalDecimal(value))

private fun growthInput(
    weight: String, weightUnit: UByte,
    length: String, lengthUnit: UByte,
    head: String, headUnit: UByte,
): GrowthInputRow = GrowthInputRow(
    weight.trim().takeIf { it.isNotEmpty() }?.let { EnteredMeasureRow(canonicalDecimal(it), weightUnit) },
    length.trim().takeIf { it.isNotEmpty() }?.let { EnteredMeasureRow(canonicalDecimal(it), lengthUnit) },
    head.trim().takeIf { it.isNotEmpty() }?.let { EnteredMeasureRow(canonicalDecimal(it), headUnit) },
)

private fun localizedEntered(context: Context, value: String): String =
    localizedDecimal(value, DecimalFormatSymbols.getInstance(context.resources.configuration.locales[0]).decimalSeparator)

private fun ChildRow.birthDateString(): String = birthDay?.let { day ->
    runCatching { LocalDate.ofEpochDay(day).toString() }.getOrNull()
}.orEmpty()

private fun growthUnitLabel(context: Context, unit: UByte): String = context.getString(when (unit) {
    10u.toUByte() -> R.string.unit_g
    11u.toUByte() -> R.string.unit_kg
    12u.toUByte() -> R.string.unit_lb
    13u.toUByte() -> R.string.unit_oz_mass
    20u.toUByte() -> R.string.unit_mm
    21u.toUByte() -> R.string.unit_cm
    else -> R.string.unit_in
})

private fun growthDisplay(context: Context, base: Long?, entered: String?, unit: UByte?,
                          baseUnit: UByte): String? = base?.let {
    val known = when (baseUnit) {
        10u.toUByte() -> massUnits.any { it.first == unit }
        else -> lengthUnits.any { it.first == unit }
    }
    val shownUnit = if (known) unit!! else baseUnit
    "${localizedEntered(context, if (known) entered ?: it.toString() else it.toString())} ${growthUnitLabel(context, shownUnit)}"
}

@Composable
private fun GrowthUnitChoices(
    title: Int,
    units: List<Pair<UByte, Int>>,
    selected: UByte,
    onSelect: (UByte) -> Unit,
) {
    Text(stringResource(title))
    units.chunked(2).forEach { row ->
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            row.forEach { (unit, label) ->
                FilterChip(
                    selected = selected == unit,
                    onClick = { onSelect(unit) },
                    label = { Text(stringResource(label)) },
                )
            }
        }
    }
}

private val massUnits = listOf(
    10u.toUByte() to R.string.unit_g,
    11u.toUByte() to R.string.unit_kg,
    12u.toUByte() to R.string.unit_lb,
    13u.toUByte() to R.string.unit_oz_mass,
)
private val lengthUnits = listOf(
    20u.toUByte() to R.string.unit_mm,
    21u.toUByte() to R.string.unit_cm,
    22u.toUByte() to R.string.unit_in,
)

private data class PendingBreastEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val startUtcMs: Long,
    val startOffsetMinutes: Short,
    val segments: List<Pair<UByte, String>>,
    // Keep pauses from a feed recorded on another client when correcting durations.
    val gapsMs: List<Long>,
)
private data class PendingDiaperEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val kind: UByte,
)
private data class PendingSolidsEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val foods: String,
    val amount: String,
)
private data class PendingGrowthEdit(
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
private data class PendingPumpEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val left: String,
    val right: String,
    val total: String,
)
private data class PendingMedicationEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val name: String,
    val doseAmount: String,
    val doseUnit: String,
)
private data class PendingChildProfileEdit(
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
private data class PendingSleepEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val startUtcMs: Long,
    val minutes: String,
)
private data class PendingSleepPlaceEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val place: UByte?,
)
private data class PendingTemperatureEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val entered: String,
    val unit: UByte,
)
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

private data class PendingInvitationCancel(
    val family: FamilyRef,
    val invitationId: ByteArray,
    val localManager: Boolean,
)

private data class PendingDeviceRemoval(
    val family: FamilyRef,
    val invitationId: ByteArray,
    val deviceId: ByteArray,
    val localManager: Boolean,
)

private data class PendingRoleChange(
    val family: FamilyRef,
    val targetId: ByteArray,
    val newRole: UByte,
    val localManager: Boolean,
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
    val activeLocal = local.filterNot { candidate ->
        removedLocal.any { it.familyId.contentEquals(candidate.familyId) }
    }
    val allRecipients = sharing.recipientFamilies()
    val removedRecipients = allRecipients.filter(sharing::isRemoved)
    val recipients = allRecipients.filterNot { candidate ->
        removedRecipients.any { it.familyId.contentEquals(candidate.familyId) }
    }
    val recipient = recipients.find { it.familyId.key() == selectedRecipient } ?: recipients.firstOrNull()
    val readyJoined = recipients.mapNotNull { candidate ->
        runCatching { candidate to sharing.snapshot(candidate) }.getOrNull()
    }
    val familyChildNames = mutableMapOf<String, String>()
    for (candidate in activeLocal) {
        val firstChild = runCatching {
            if (sharing.isShared(candidate)) sharing.snapshot(candidate).children.firstOrNull()?.name
            else store.children(candidate).firstOrNull()?.name
        }.getOrNull()
        if (firstChild != null) familyChildNames[candidate.familyId.key()] = firstChild
    }
    for ((candidate, snapshot) in readyJoined) {
        snapshot.children.firstOrNull()?.name?.let { familyChildNames[candidate.familyId.key()] = it }
    }
    val joinedSnapshot = readyJoined.find {
        it.first.familyId.key() == recipient?.familyId?.key()
    }?.second
    val shown = activeLocal + readyJoined.map { it.first }
    val family = shown.find { it.familyId.key() == selectedFamily } ?: shown.firstOrNull()
    val localFamily = family != null && activeLocal.any { it.familyId.key() == family.familyId.key() }
    val recipientSnapshot = readyJoined.find { it.first.familyId.key() == family?.familyId?.key() }?.second
    val shared = recipientSnapshot != null || (family?.let(sharing::isShared) ?: false)
    val snapshot = recipientSnapshot ?: if (shared) sharing.snapshot(family ?: error("Shared Family absent")) else null
    val unusedInvitationIds = if (family != null && snapshot?.devices?.any {
        it.deviceId.contentEquals(family.deviceId) && it.role == 2.toUByte()
    } == true) runCatching { sharing.unusedInvitationIds(family) }.getOrNull() else emptyList()
    val kids = snapshot?.children ?: family?.let(store::children).orEmpty()
    val child = kids.find { it.id.key() == selectedChild } ?: kids.firstOrNull()
    val history = if (family != null && child != null) {
        snapshot?.activities?.filter { it.childId.contentEquals(child.id) }
            ?: store.timeline(family, child.id)
    } else emptyList()
    val summaryZone = ZoneId.systemDefault()
    val summaryDay = LocalDate.now(summaryZone)
    val window = DayWindowRow(
        summaryDay.atStartOfDay(summaryZone).toInstant().toEpochMilli(),
        summaryDay.plusDays(1).atStartOfDay(summaryZone).toInstant().toEpochMilli(),
        System.currentTimeMillis(),
    )
    val daySummary = if (family != null && child != null) {
        if (shared) sharing.daySummary(family, child.id, window)
        else store.daySummary(family, child.id, window)
    } else null
    return ScreenData(
        shown, (removedLocal + removedRecipients).distinctBy { it.familyId.key() },
        familyChildNames, family?.familyId?.key(), child?.id?.key(), localFamily, kids, history,
        if (!shared) family?.let(store::revision) ?: 0uL else 0uL,
        if (!shared) family?.let(store::restoredOrigin) else null,
        shared, snapshot, recipients, readyJoined.mapTo(mutableSetOf()) { it.first.familyId.key() },
        joinedSnapshot, unusedInvitationIds,
        runningSleepCount(store, sharing, activeLocal, recipients),
        daySummary, if (daySummary != null) summaryDay else null,
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TrackerScreen(
    store: NativeLocalStore,
    sharing: ShareCoordinator,
    incomingInvitation: String?,
    onInvitationConsumed: () -> Unit,
    readFile: (android.net.Uri) -> ByteArray?,
    writeFile: (android.net.Uri, ByteArray) -> Unit,
    availableMemory: () -> Long,
    lastSave: (FamilyRef) -> CompletedSave?,
    recordSave: (BackupFileRow) -> Boolean,
    lastRelayOrigin: (FamilyRef) -> String?,
    recordRelayOrigin: (FamilyRef, String) -> Boolean,
) {
    val context = LocalContext.current
    val activity = context as ComponentActivity
    var foreground by remember { mutableStateOf(activity.lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)) }
    DisposableEffect(activity) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_START) foreground = true
            if (event == Lifecycle.Event.ON_STOP) foreground = false
        }
        activity.lifecycle.addObserver(observer)
        onDispose { activity.lifecycle.removeObserver(observer) }
    }
    val scope = rememberCoroutineScope()
    var version by remember { mutableStateOf(0) }
    val notificationPermission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
        if (it) version++
    }
    var families by remember { mutableStateOf<List<FamilyRef>>(emptyList()) }
    var removedFamilies by remember { mutableStateOf<List<FamilyRef>>(emptyList()) }
    var familyChildNames by remember { mutableStateOf<Map<String, String>>(emptyMap()) }
    var children by remember { mutableStateOf<List<ChildRow>>(emptyList()) }
    var entries by remember { mutableStateOf<List<ActivityRow>>(emptyList()) }
    var daySummary by remember { mutableStateOf<DaySummaryRow?>(null) }
    var daySummaryDay by remember { mutableStateOf<LocalDate?>(null) }
    var revision by remember { mutableStateOf(0uL) }
    var restoredOrigin by remember { mutableStateOf<RestoredOriginRow?>(null) }
    var isShared by remember { mutableStateOf(false) }
    var activeSharedSnapshot by remember { mutableStateOf<SharedSnapshotRow?>(null) }
    var activeUnusedInvitationIds by remember { mutableStateOf<List<ByteArray>?>(emptyList()) }
    var loadedFamilyKey by remember { mutableStateOf<String?>(null) }
    var loadedChildKey by remember { mutableStateOf<String?>(null) }
    var activeFamilyIsLocal by remember { mutableStateOf(false) }
    var saveStatusVersion by remember { mutableStateOf(0) }
    val selectionPrefs = remember { context.getSharedPreferences("tracker_selection", Context.MODE_PRIVATE) }
    val removalNoticePrefs = remember {
        context.getSharedPreferences("acknowledged_removal_copies", Context.MODE_PRIVATE)
    }
    var selectedFamily by remember { mutableStateOf(selectionPrefs.getString("family", null)) }
    var selectedChild by remember { mutableStateOf(selectionPrefs.getString("child", null)) }
    var destination by rememberSaveable { mutableStateOf(TrackerDestination.TODAY) }
    var captureKind by rememberSaveable { mutableStateOf<CaptureKind?>(null) }
    var showTargetPicker by remember { mutableStateOf(false) }
    var pendingRemovalNotice by remember { mutableStateOf<RemovalCopyNotice?>(null) }
    LaunchedEffect(foreground, version, pendingRemovalNotice) {
        if (!foreground || pendingRemovalNotice != null) return@LaunchedEffect
        runCatching {
            withContext(Dispatchers.IO) {
                (store.families() + sharing.recipientFamilies())
                    .distinctBy { it.familyId.key() }
                    .firstNotNullOfOrNull { source ->
                        val copy = sharing.savedRemovalCopy(source) ?: return@firstNotNullOfOrNull null
                        val sourceKey = source.familyId.key()
                        if (removalNoticePrefs.getString(sourceKey, null) == copy.familyId.key()) null
                        else RemovalCopyNotice(sourceKey, copy)
                    }
            }
        }.onSuccess { pendingRemovalNotice = it }
            .onFailure { Log.w("BabytrackRemoval", "Could not read saved copy destination", it) }
    }
    LaunchedEffect(loadedFamilyKey, selectedFamily, selectedChild, children) {
        if (selectedFamily != null && loadedFamilyKey == selectedFamily) {
            val child = selectedChild.takeIf { chosen -> children.any { it.id.key() == chosen } }
            selectionPrefs.edit().putString("family", selectedFamily).putString("child", child).apply()
        }
    }
    var childName by remember { mutableStateOf("") }
    var showAddChildForm by remember { mutableStateOf(false) }
    var showChildDetails by remember { mutableStateOf(false) }
    var pendingChildProfileEdit by remember { mutableStateOf<PendingChildProfileEdit?>(null) }
    var childBirthDate by remember { mutableStateOf("") }
    var childSex by remember { mutableStateOf(3u.toUByte()) }
    var childProfileSaving by remember { mutableStateOf(false) }
    var amount by remember { mutableStateOf("") }
    var bottleUnit by remember { mutableStateOf(1u.toUByte()) }
    var bottleContent by remember { mutableStateOf(2u.toUByte()) }
    var breastMinutes by remember { mutableStateOf("") }
    var breastSide by remember { mutableStateOf(1u.toUByte()) }
    var breastDraftSegments by remember { mutableStateOf<List<Pair<UByte, Long>>>(emptyList()) }
    var pumpMinutes by remember { mutableStateOf("") }
    var pumpLeft by remember { mutableStateOf("") }
    var pumpRight by remember { mutableStateOf("") }
    var pumpTotal by remember { mutableStateOf("") }
    var solidsFoods by remember { mutableStateOf("") }
    var solidsAmount by remember { mutableStateOf("") }
    var sleepMinutes by remember { mutableStateOf("") }
    var sleepPlace by remember { mutableStateOf<UByte?>(null) }
    var noteText by remember { mutableStateOf("") }
    var growthWeight by remember { mutableStateOf("") }
    var growthWeightUnit by remember { mutableStateOf(11u.toUByte()) }
    var growthLength by remember { mutableStateOf("") }
    var growthLengthUnit by remember { mutableStateOf(21u.toUByte()) }
    var growthHead by remember { mutableStateOf("") }
    var growthHeadUnit by remember { mutableStateOf(21u.toUByte()) }
    var temperatureEntered by remember { mutableStateOf("") }
    var temperatureUnit by remember { mutableStateOf(30u.toUByte()) }
    var medicationName by remember { mutableStateOf("") }
    var doseAmount by remember { mutableStateOf("") }
    var doseUnit by remember { mutableStateOf("") }
    var logAtMs by remember { mutableStateOf<Long?>(null) }
    var timelineFilter by remember { mutableStateOf(TimelineFilter.ALL) }
    var selectedHistoryDay by rememberSaveable { mutableStateOf<String?>(null) }
    var expandedEntryKey by remember { mutableStateOf<String?>(null) }
    var message by remember { mutableStateOf<String?>(null) }
    val snackbarHostState = remember { SnackbarHostState() }
    LaunchedEffect(message) {
        message?.let { snackbarHostState.showSnackbar(it) }
    }
    var automaticSyncDelayed by remember { mutableStateOf(false) }
    var automaticSyncBlocked by remember { mutableStateOf(false) }
    var removalTarget by remember { mutableStateOf<ByteArray?>(null) }
    var cancelInvitationTarget by remember { mutableStateOf<PendingInvitationCancel?>(null) }
    var pendingDeviceRemoval by remember { mutableStateOf<PendingDeviceRemoval?>(null) }
    var roleChangeTarget by remember { mutableStateOf<PendingRoleChange?>(null) }
    val deviceLabelPrefs = remember { context.getSharedPreferences("device_labels", Context.MODE_PRIVATE) }
    var deviceLabels by remember {
        mutableStateOf(deviceLabelPrefs.all.mapNotNull { (key, value) ->
            (value as? String)?.let { key to it }
        }.toMap())
    }
    var deviceLabelTarget by remember { mutableStateOf<String?>(null) }
    var deviceLabelDraft by remember { mutableStateOf("") }
    var pendingDelete by remember { mutableStateOf<PendingActivityDelete?>(null) }
    var recentlyDeleted by remember { mutableStateOf<PendingActivityDelete?>(null) }
    LaunchedEffect(recentlyDeleted) {
        val target = recentlyDeleted ?: return@LaunchedEffect
        val result = snackbarHostState.showSnackbar(
            context.getString(R.string.entry_deleted),
            actionLabel = context.getString(R.string.undo),
            duration = SnackbarDuration.Long,
        )
        if (result == SnackbarResult.ActionPerformed) {
            runCatching { withContext(Dispatchers.IO) {
                val at = System.currentTimeMillis()
                if (target.shared) sharing.restoreActivity(
                    target.family, target.childId, target.activityId, at,
                ) else store.restoreActivity(
                    target.family, target.childId, target.activityId, at,
                )
            } }.onSuccess {
                version++
                message = null
            }.onFailure { message = context.getString(R.string.error) }
        }
        recentlyDeleted = null
    }
    var pendingNoteEdit by remember { mutableStateOf<PendingNoteEdit?>(null) }
    var pendingTimeEdit by remember { mutableStateOf<PendingTimeEdit?>(null) }
    var pendingBottleEdit by remember { mutableStateOf<PendingBottleEdit?>(null) }
    var pendingBreastEdit by remember { mutableStateOf<PendingBreastEdit?>(null) }
    var pendingDiaperEdit by remember { mutableStateOf<PendingDiaperEdit?>(null) }
    var pendingSolidsEdit by remember { mutableStateOf<PendingSolidsEdit?>(null) }
    var pendingGrowthEdit by remember { mutableStateOf<PendingGrowthEdit?>(null) }
    var pendingPumpEdit by remember { mutableStateOf<PendingPumpEdit?>(null) }
    var pendingMedicationEdit by remember { mutableStateOf<PendingMedicationEdit?>(null) }
    var pendingSleepEdit by remember { mutableStateOf<PendingSleepEdit?>(null) }
    var pendingSleepPlaceEdit by remember { mutableStateOf<PendingSleepPlaceEdit?>(null) }
    var pendingTemperatureEdit by remember { mutableStateOf<PendingTemperatureEdit?>(null) }
    var relayOrigin by remember { mutableStateOf("") }
    var relayPublicKey by remember { mutableStateOf("") }
    var shareStage by remember { mutableStateOf<String?>(null) }
    var shareInProgress by remember { mutableStateOf(false) }
    var inviteInProgress by remember { mutableStateOf(false) }
    var invitationFragment by remember { mutableStateOf<String?>(null) }
    var receivedFragment by remember { mutableStateOf("") }
    var showJoinForm by remember { mutableStateOf(false) }
    var showShareForm by remember { mutableStateOf(false) }
    var showFamilySetup by remember { mutableStateOf(false) }
    var showAccessControls by remember { mutableStateOf(false) }
    var showDataControls by remember { mutableStateOf(false) }
    var joinStage by remember { mutableStateOf<String?>(null) }
    var joinInProgress by remember { mutableStateOf(false) }
    var sharedSnapshot by remember { mutableStateOf<SharedSnapshotRow?>(null) }
    var recipientFamilies by remember { mutableStateOf<List<FamilyRef>>(emptyList()) }
    var readyRecipientKeys by remember { mutableStateOf<Set<String>>(emptySet()) }
    var selectedRecipient by remember { mutableStateOf<String?>(null) }
    var inviteAsManager by remember { mutableStateOf(false) }
    LaunchedEffect(incomingInvitation) {
        if (incomingInvitation != null) {
            receivedFragment = incomingInvitation
            showJoinForm = true
            destination = TrackerDestination.FAMILY
        }
    }
    val errorText = stringResource(R.string.error)
    val savedText = stringResource(R.string.saved)
    val restoredText = stringResource(R.string.restored)
    LaunchedEffect(selectedFamily) {
        showAccessControls = false
        showDataControls = false
        showShareForm = false
        shareStage = null
        invitationFragment = null
        relayPublicKey = ""
        inviteAsManager = false
        childName = ""
        childBirthDate = ""
        childSex = 3u.toUByte()
        val family = families.find { it.familyId.key() == selectedFamily }
        relayOrigin = family?.let {
            if (recipientFamilies.any { recipient -> recipient.familyId.key() == it.familyId.key() }) {
                runCatching { sharing.recipientOrigin(it) }.getOrNull()
            } else lastRelayOrigin(it)
        }.orEmpty()
    }
    LaunchedEffect(selectedFamily, selectedChild) {
        showTargetPicker = false
        captureKind = null
        if (destination == TrackerDestination.CAPTURE) destination = TrackerDestination.TODAY
        expandedEntryKey = null
        recentlyDeleted = null
        showChildDetails = false
        showAddChildForm = false
        childName = ""
        childBirthDate = ""
        childSex = 3u.toUByte()
        amount = ""
        bottleUnit = 1u.toUByte()
        bottleContent = 2u.toUByte()
        breastDraftSegments = emptyList()
        breastMinutes = ""
        breastSide = 1u.toUByte()
        pumpMinutes = ""
        pumpLeft = ""
        pumpRight = ""
        pumpTotal = ""
        solidsFoods = ""
        solidsAmount = ""
        sleepMinutes = ""
        sleepPlace = null
        noteText = ""
        growthWeight = ""
        growthWeightUnit = 11u.toUByte()
        growthLength = ""
        growthLengthUnit = 21u.toUByte()
        growthHead = ""
        growthHeadUnit = 21u.toUByte()
        temperatureEntered = ""
        temperatureUnit = 30u.toUByte()
        medicationName = ""
        doseAmount = ""
        doseUnit = ""
        pendingChildProfileEdit = null
        childProfileSaving = false
        pendingDelete = null
        pendingNoteEdit = null
        pendingTimeEdit = null
        pendingBottleEdit = null
        pendingBreastEdit = null
        pendingDiaperEdit = null
        pendingSolidsEdit = null
        pendingGrowthEdit = null
        pendingPumpEdit = null
        pendingMedicationEdit = null
        pendingSleepEdit = null
        pendingSleepPlaceEdit = null
        pendingTemperatureEdit = null
        logAtMs = null
        timelineFilter = TimelineFilter.ALL
        selectedHistoryDay = null
    }
    var pendingBackup by remember { mutableStateOf<BackupFileRow?>(null) }
    var pendingAnalysisCsv by remember { mutableStateOf<ByteArray?>(null) }
    var protectBackup by remember { mutableStateOf(false) }
    var backupPassword by remember { mutableStateOf("") }
    var restorePassword by remember { mutableStateOf("") }
    var pendingRestore by remember { mutableStateOf<ByteArray?>(null) }
    var pendingRestoreProtected by remember { mutableStateOf(false) }
    var restoreInfo by remember { mutableStateOf<BackupInfoRow?>(null) }
    val passwordNeeded = stringResource(R.string.password_needed)
    val passwordOrFileError = stringResource(R.string.password_or_file_error)
    val damagedBackupError = stringResource(R.string.damaged_backup_error)
    LaunchedEffect(foreground, selectedRecipient) {
        if (foreground) while (isActive) {
            val (syncResult, terminalReasons, managerRemovals) = withContext(Dispatchers.IO) {
                var failed = false
                var blocked = false
                val stages = mutableMapOf<String, uniffi.babytrack_core_ffi.RecipientSyncRow>()
                val terminals = mutableMapOf<String, InvitationTerminalReason>()
                val removals = mutableMapOf<String, RemovedDeviceRow>()
                val localFamilies = runCatching { store.families() }.getOrElse {
                    if (it is kotlinx.coroutines.CancellationException) throw it
                    Log.w("BabytrackSync", "Could not list local Families", it)
                    failed = true
                    emptyList()
                }
                for (family in localFamilies) {
                    val origin = lastRelayOrigin(family) ?: continue
                    val canAdvance = runCatching {
                        sharing.isShared(family) && !sharing.isRemoved(family)
                    }.getOrElse {
                        if (it is kotlinx.coroutines.CancellationException) throw it
                        Log.w("BabytrackSync", "Could not read shared Family state", it)
                        failed = true
                        false
                    }
                    if (canAdvance) {
                        runCatching { sharing.advanceManager(family, origin) }
                            .onFailure {
                                if (it is kotlinx.coroutines.CancellationException) throw it
                                if (it is VerifiedManagerRemoval) removals[family.familyId.key()] = it.result
                                else { failed = true; if (it is SharedUploadBlocked) blocked = true }
                            }
                    }
                }
                val recipients = runCatching { sharing.recipientFamilies() }.getOrElse {
                    if (it is kotlinx.coroutines.CancellationException) throw it
                    Log.w("BabytrackSync", "Could not list joining Families", it)
                    failed = true
                    emptyList()
                }
                for (family in recipients) {
                    runCatching { sharing.advanceRecipient(family) }
                        .onSuccess { stages[family.familyId.key()] = it }
                        .onFailure {
                            if (it is kotlinx.coroutines.CancellationException) throw it
                            if (it is InvitationTerminal) terminals[family.familyId.key()] = it.reason
                            else { failed = true; if (it is SharedUploadBlocked) blocked = true }
                        }
                }
                Triple(Triple(failed, blocked, stages), terminals, removals)
            }
            automaticSyncDelayed = syncResult.first
            automaticSyncBlocked = syncResult.second
            val recipientStages = syncResult.third
            terminalReasons[selectedRecipient]?.let { reason ->
                joinStage = terminalInvitationMessage(context, reason)
                showJoinForm = true
                sharedSnapshot = null
            }
            managerRemovals[selectedFamily]?.let { removed ->
                val copy = removed.privateCopy
                if (copy != null) {
                    pendingRemovalNotice = RemovalCopyNotice(selectedFamily ?: return@let, copy)
                    message = when (removed.pendingResult) {
                        2.toUByte() -> context.getString(R.string.history_removed_accepted)
                        3.toUByte() -> context.getString(R.string.history_removed_rejected)
                        0.toUByte() -> context.getString(R.string.history_removed_unsent)
                        else -> context.getString(R.string.history_removed_copied)
                    }
                } else message = context.getString(R.string.history_removed)
            }
            recipientStages[selectedRecipient]?.let { progress ->
                if (progress.removed) sharedSnapshot = null
                joinStage = when {
                    progress.removed -> removedHistoryMessage(context, progress)
                    progress.joinPhase == 8u.toUByte() -> context.getString(R.string.pending_join_removed)
                    progress.ready -> context.getString(R.string.history_ready_auto)
                    progress.awaitingGrant -> pendingRecipientMessage(context, progress)
                    else -> context.getString(R.string.history_pending, progress.verifiedCursor.toLong())
                }
            }
            version++
            delay(30_000)
        }
    }
    val tooLargeError = stringResource(R.string.backup_too_large)
    val saveLauncher = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri ->
        if (uri != null) scope.launch {
            runCatching { withContext(Dispatchers.IO) {
                val file = pendingBackup ?: error("Missing backup")
                writeFile(uri, file.bytes)
                check(recordSave(file))
            } }
                .onSuccess { message = savedText; saveStatusVersion++ }
                .onFailure { message = errorText }
            pendingBackup = null
        }
    }
    val csvLauncher = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("text/csv")) { uri ->
        if (uri != null) scope.launch {
            runCatching { withContext(Dispatchers.IO) {
                writeFile(uri, pendingAnalysisCsv ?: error("Missing analysis export"))
            } }
                .onSuccess { message = context.getString(R.string.analysis_csv_saved) }
                .onFailure { message = errorText }
            pendingAnalysisCsv = null
        } else pendingAnalysisCsv = null
    }
    val restoreLauncher = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) scope.launch {
            pendingRestore = null
            restoreInfo = null
            restorePassword = ""
            pendingRestoreProtected = false
            val bytes = runCatching { withContext(Dispatchers.IO) { readFile(uri) ?: error("Missing backup") } }
                .getOrElse {
                    message = if (it is BackupTooLarge) tooLargeError else errorText
                    return@launch
                }
            val protected = bytes.size >= 5 && bytes.copyOfRange(0, 5).contentEquals("BTBK1".toByteArray())
            if (protected) {
                pendingRestore = bytes
                pendingRestoreProtected = true
                message = passwordNeeded
            } else {
                runCatching { withContext(Dispatchers.IO) { store.inspectReadable(bytes) } }
                    .onSuccess { info ->
                        pendingRestore = bytes
                        restoreInfo = info
                        message = null
                    }
                    .onFailure { message = damagedBackupError }
            }
        }
    }
    fun change(onSaved: (() -> Unit)? = null, action: () -> Unit) {
        scope.launch {
            runCatching { withContext(Dispatchers.IO) { action() } }
                .onSuccess { version++; message = null; onSaved?.invoke() }
                .onFailure { message = errorText }
        }
    }
    LaunchedEffect(version, selectedFamily, selectedChild, selectedRecipient) {
        runCatching {
            withContext(Dispatchers.IO) {
                loadTrackerData(store, sharing, selectedFamily, selectedChild, selectedRecipient)
            }
        }.onSuccess { data ->
            val all = data.families
            val kids = data.children
            families = all
            removedFamilies = data.removedFamilies
            familyChildNames = data.familyChildNames
            selectedFamily = all.find { it.familyId.key() == selectedFamily }?.familyId?.key() ?: all.firstOrNull()?.familyId?.key()
            activeFamilyIsLocal = data.activeFamilyIsLocal
            children = kids
            selectedChild = kids.find { it.id.key() == selectedChild }?.id?.key() ?: kids.firstOrNull()?.id?.key()
            entries = data.entries
            daySummary = data.daySummary
            daySummaryDay = data.daySummaryDay
            revision = data.revision
            restoredOrigin = data.restoredOrigin
            isShared = data.shared
            activeSharedSnapshot = data.mainSharedSnapshot
            activeUnusedInvitationIds = data.unusedInvitationIds
            loadedFamilyKey = data.activeFamilyKey
            loadedChildKey = data.activeChildKey
            recipientFamilies = data.recipients
            readyRecipientKeys = data.readyRecipientKeys
            selectedRecipient = data.recipients.find { it.familyId.key() == selectedRecipient }
                ?.familyId?.key() ?: data.recipients.firstOrNull()?.familyId?.key()
            sharedSnapshot = data.joinedSnapshot
            runCatching { SleepTimerNotifications.update(context, data.activeSleepCount) }
                .onFailure { android.util.Log.w("BabytrackTimer", "Could not update sleep notification", it) }
        }.onFailure {
            if (it is kotlinx.coroutines.CancellationException) throw it
            Log.e("BabytrackTracker", "Could not load tracker", it)
            message = errorText
        }
    }
    val family = families.find { it.familyId.key() == selectedFamily }
    val child = children.find { it.id.key() == selectedChild }
        ?.takeIf { loadedFamilyKey == selectedFamily }
    val activeShared = isShared && loadedFamilyKey == selectedFamily
    val completed = remember(selectedFamily, saveStatusVersion) { family?.let(lastSave) }
    val filename = stringResource(R.string.backup_filename)
    val protectedFilename = stringResource(R.string.protected_backup_filename)
    val todayScrollState = rememberScrollState()
    val historyScrollState = rememberScrollState()
    val familyScrollState = rememberScrollState()
    val captureScrollState = rememberScrollState()
    LaunchedEffect(selectedFamily, selectedChild) {
        todayScrollState.scrollTo(0)
        historyScrollState.scrollTo(0)
        familyScrollState.scrollTo(0)
        captureScrollState.scrollTo(0)
    }
    val route = if (family == null || child == null) TrackerDestination.FAMILY else destination
    val scrollState = when (route) {
        TrackerDestination.TODAY -> todayScrollState
        TrackerDestination.HISTORY -> historyScrollState
        TrackerDestination.FAMILY -> familyScrollState
        TrackerDestination.CAPTURE -> captureScrollState
    }
    BackHandler(enabled = route != TrackerDestination.TODAY && family != null && child != null) {
        destination = TrackerDestination.TODAY
        captureKind = null
    }
    LaunchedEffect(incomingInvitation) {
        if (incomingInvitation != null) familyScrollState.scrollTo(0)
    }
    val joinFirst = incomingInvitation != null &&
        (receivedFragment.isNotBlank() || sharedSnapshot == null)
    fun logTime(): ActivityWhen = activityWhen(logAtMs ?: System.currentTimeMillis())
    fun resetLogTime(savedAt: Long?) {
        if (logAtMs == savedAt) logAtMs = null
    }
    fun finishCapture() {
        if (destination == TrackerDestination.CAPTURE) {
            destination = TrackerDestination.TODAY
            captureKind = null
        }
    }
    fun logCompleted(onSaved: (() -> Unit)? = null, action: (ActivityWhen) -> Unit) {
        val chosenAt = logAtMs
        val at = logTime()
        change(onSaved = {
            resetLogTime(chosenAt)
            onSaved?.invoke()
            finishCapture()
        }) { action(at) }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = {
                    if (route == TrackerDestination.CAPTURE) {
                        IconButton(onClick = {
                            destination = TrackerDestination.TODAY
                            captureKind = null
                        }) { Icon(painterResource(R.drawable.ic_back), contentDescription = stringResource(R.string.back)) }
                    }
                },
                title = {
                    val familyNumber = families.indexOfFirst { it.familyId.key() == selectedFamily } + 1
                    val title = if (child != null && familyNumber > 0) {
                        stringResource(R.string.family_with_child, familyNumber, child.name)
                    } else stringResource(when (route) {
                        TrackerDestination.TODAY -> R.string.nav_today
                        TrackerDestination.HISTORY -> R.string.nav_history
                        TrackerDestination.FAMILY -> R.string.nav_family
                        TrackerDestination.CAPTURE -> R.string.add_activity
                    })
                    if (child != null && route != TrackerDestination.CAPTURE) {
                        val description = stringResource(R.string.switch_target)
                        TextButton(onClick = { showTargetPicker = true },
                            modifier = Modifier.semantics { contentDescription = description }) {
                            Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis,
                                color = MaterialTheme.colorScheme.onSurface,
                                style = MaterialTheme.typography.titleLarge)
                            Icon(painterResource(R.drawable.ic_expand_more), contentDescription = null)
                        }
                    } else Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis)
                },
            )
        },
        bottomBar = {
            if (family != null && child != null && route != TrackerDestination.CAPTURE) {
                NavigationBar {
                    listOf(
                        Triple(TrackerDestination.TODAY, R.string.nav_today, R.drawable.ic_today),
                        Triple(TrackerDestination.HISTORY, R.string.nav_history, R.drawable.ic_history),
                        Triple(TrackerDestination.FAMILY, R.string.nav_family, R.drawable.ic_family),
                    ).forEach { (target, label, icon) ->
                        NavigationBarItem(
                            selected = route == target,
                            onClick = { destination = target },
                            icon = { Icon(painterResource(icon), contentDescription = null) },
                            label = { Text(stringResource(label)) },
                        )
                    }
                }
            }
        },
        snackbarHost = { SnackbarHost(snackbarHostState) },
    ) { padding ->
        val joinControls: @Composable () -> Unit = {
            if (!showJoinForm && recipientFamilies.isEmpty()) OutlinedButton(
                onClick = { showJoinForm = true },
            ) { Text(stringResource(R.string.join_family)) }
            else Card(modifier = Modifier.fillMaxWidth()) {
                Column(
                    modifier = Modifier.padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Text(stringResource(R.string.join_family), style = MaterialTheme.typography.titleMedium)
                    Text(stringResource(R.string.join_description))
                    recipientFamilies.forEachIndexed { index, recipient ->
                        val ready = recipient.familyId.key() in readyRecipientKeys
                        FilterChip(
                            selected = recipient.familyId.key() == selectedRecipient,
                            onClick = {
                                selectedRecipient = recipient.familyId.key()
                                joinStage = null
                            },
                            label = { Text(stringResource(if (ready) R.string.ready_family_number
                                else R.string.joining_family_number, index + 1)) },
                        )
                    }
                    if (selectedRecipient != null && selectedRecipient !in readyRecipientKeys) {
                        Text(stringResource(R.string.saved_join_pending))
                    }
                    OutlinedTextField(
                        value = receivedFragment,
                        onValueChange = { receivedFragment = it },
                        label = { Text(stringResource(R.string.received_fragment)) },
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                    )
                    Button(enabled = !joinInProgress &&
                        (receivedFragment.isNotBlank() || selectedRecipient != null &&
                            sharedSnapshot?.family?.familyId?.key() != selectedRecipient), onClick = {
                        joinInProgress = true
                        joinStage = context.getString(R.string.join_preparing)
                        scope.launch {
                            runCatching {
                                withContext(Dispatchers.IO) {
                                    val recipient = recipientFamilies.find { it.familyId.key() == selectedRecipient }
                                    val prepared = if (receivedFragment.isNotBlank()) sharing.claim(receivedFragment.trim())
                                        else sharing.retryClaim(recipient ?: error("No saved recipient claim"))
                                    prepared to runCatching { sharing.advanceRecipient(prepared.family) }
                                }
                            }.onSuccess { (prepared, result) ->
                                selectedRecipient = prepared.family.familyId.key()
                                receivedFragment = ""
                                onInvitationConsumed()
                                version++
                                val progress = result.getOrNull()
                                joinStage = when {
                                    progress?.ready == true -> context.getString(R.string.history_ready_auto)
                                    progress?.joinPhase == 8u.toUByte() -> context.getString(R.string.pending_join_removed)
                                    progress?.awaitingGrant == true -> pendingRecipientMessage(context, progress)
                                    progress != null -> context.getString(R.string.history_pending, progress.verifiedCursor.toLong())
                                    result.exceptionOrNull() is InvitationTerminal -> terminalInvitationMessage(
                                        context, (result.exceptionOrNull() as InvitationTerminal).reason)
                                    else -> context.getString(R.string.join_progress_delayed)
                                }
                                message = null
                            }.onFailure { failure ->
                                joinStage = (failure as? InvitationTerminal)?.reason
                                    ?.let { terminalInvitationMessage(context, it) }
                                    ?: context.getString(R.string.join_retry)
                                message = if (failure is InvitationTerminal) null else errorText
                                version++
                            }
                            joinInProgress = false
                        }
                    }) { Text(stringResource(R.string.join_or_retry)) }
                    joinStage?.let { Text(it) }
                }
            }
        }
        Column(
            modifier = Modifier.fillMaxSize().padding(padding).verticalScroll(scrollState).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            if (route == TrackerDestination.FAMILY) {
            if (joinFirst) joinControls()
            if (family == null && !joinFirst) Text(stringResource(R.string.first_run_intro))
            if (family != null) Text(
                stringResource(if (activeShared) R.string.shared_family else R.string.local_only),
                style = MaterialTheme.typography.labelMedium,
            )
            if (automaticSyncDelayed && !automaticSyncBlocked) Text(stringResource(R.string.automatic_sync_delayed))
            if (automaticSyncBlocked) Text(
                stringResource(R.string.shared_upload_blocked),
                color = MaterialTheme.colorScheme.error,
            )
            if (activeShared) {
                shareStage?.let { Text(it) }
                activeSharedSnapshot?.let { snapshot ->
                    Text(pluralStringResource(R.plurals.shared_device_count,
                        snapshot.devices.size, snapshot.devices.size))
                    if (snapshot.pendingDevices.isNotEmpty()) {
                        Text(pluralStringResource(R.plurals.shared_pending_device_count,
                            snapshot.pendingDevices.size, snapshot.pendingDevices.size))
                    }
                    if (snapshot.unsentCount > 0uL) {
                        Text(stringResource(R.string.shared_pending_changes, snapshot.unsentCount.toLong()))
                    }
                    if (snapshot.inertCount > 0uL) {
                        Text(stringResource(R.string.shared_unreadable_batches, snapshot.inertCount.toLong()),
                            color = MaterialTheme.colorScheme.error)
                    }
                }
                OutlinedButton(onClick = { showAccessControls = !showAccessControls }) {
                    Text(stringResource(if (showAccessControls) R.string.hide_family_access
                        else R.string.show_family_access))
                }
            }
            if (activeShared && showAccessControls) activeSharedSnapshot?.let { snapshot ->
                SharedHealth(snapshot, deviceLabels, onNameDevice = { target ->
                    val key = deviceLabelKey(snapshot.family.familyId, target)
                    deviceLabelTarget = key
                    deviceLabelDraft = deviceLabels[key].orEmpty()
                })
                if (!activeFamilyIsLocal) {
                    Text(stringResource(R.string.shared_manual_sync))
                    OutlinedButton(onClick = {
                        scope.launch {
                            runCatching { withContext(Dispatchers.IO) {
                                sharing.syncRecipientAndUpload(snapshot.family)
                            } }.onSuccess { progress ->
                                version++
                                message = sharedSyncMessage(context, progress)
                            }.onFailure {
                                message = if (it is SharedUploadBlocked)
                                    context.getString(R.string.shared_upload_blocked) else errorText
                            }
                        }
                    }) { Text(stringResource(R.string.sync_shared)) }
                }
                if (family != null && snapshot.devices.any {
                    it.deviceId.contentEquals(family.deviceId) && it.role == 2.toUByte()
                }) {
                    snapshot.devices.filterNot { it.deviceId.contentEquals(family.deviceId) }.forEach { device ->
                        val label = deviceLabels[deviceLabelKey(snapshot.family.familyId, device.deviceId)]
                            ?.takeIf { it.isNotBlank() }
                            ?: stringResource(R.string.device_short_id, device.deviceId.key().take(8))
                        OutlinedButton(onClick = { removalTarget = device.deviceId }) {
                            Text(stringResource(R.string.remove_device, label))
                        }
                        if (BuildConfig.DEBUG) {
                            val nextRole = if (device.role == 2.toUByte()) 1u.toUByte() else 2u.toUByte()
                            OutlinedButton(onClick = {
                                roleChangeTarget = PendingRoleChange(
                                    family, device.deviceId.copyOf(), nextRole, activeFamilyIsLocal,
                                )
                            }) {
                                Text(stringResource(if (nextRole == 2.toUByte())
                                    R.string.promote_device else R.string.demote_device, label))
                            }
                        }
                    }
                    if (snapshot.pendingDevices.isNotEmpty()) {
                        Text(stringResource(R.string.pending_devices), style = MaterialTheme.typography.titleMedium)
                        snapshot.pendingDevices.forEach { pending ->
                            val label = deviceLabels[deviceLabelKey(snapshot.family.familyId, pending.deviceId)]
                                ?.takeIf { it.isNotBlank() }
                                ?: stringResource(R.string.device_short_id, pending.deviceId.key().take(8))
                            OutlinedButton(onClick = {
                                pendingDeviceRemoval = PendingDeviceRemoval(
                                    family, pending.invitationId.copyOf(), pending.deviceId.copyOf(),
                                    activeFamilyIsLocal,
                                )
                            }) { Text(stringResource(R.string.remove_pending_device, label)) }
                        }
                    }
                    if (BuildConfig.DEBUG) {
                        if (activeUnusedInvitationIds == null) {
                            Text(stringResource(R.string.invitation_list_delayed))
                        } else if (!activeUnusedInvitationIds.isNullOrEmpty()) {
                            Text(stringResource(R.string.unused_invitations), style = MaterialTheme.typography.titleMedium)
                            activeUnusedInvitationIds.orEmpty().forEach { invitationId ->
                                OutlinedButton(onClick = {
                                    cancelInvitationTarget = PendingInvitationCancel(
                                        family, invitationId.copyOf(), activeFamilyIsLocal,
                                    )
                                }) {
                                    Text(stringResource(R.string.cancel_invitation,
                                        invitationId.key().take(8)))
                                }
                            }
                        }
                    }
                    val inviteOrigin = if (activeFamilyIsLocal) lastRelayOrigin(snapshot.family).orEmpty()
                        else runCatching { sharing.recipientOrigin(snapshot.family) }.getOrNull().orEmpty()
                    if (inviteOrigin.isNotBlank()) {
                        Text(stringResource(R.string.invite_caregiver), style = MaterialTheme.typography.titleMedium)
                        Text(stringResource(R.string.invite_description))
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            FilterChip(
                                selected = !inviteAsManager,
                                onClick = { inviteAsManager = false },
                                label = { Text(stringResource(R.string.invite_member)) },
                            )
                            FilterChip(
                                selected = inviteAsManager,
                                onClick = { inviteAsManager = true },
                                label = { Text(stringResource(R.string.invite_manager)) },
                            )
                        }
                        OutlinedButton(enabled = !inviteInProgress, onClick = {
                            val invitedFamily = snapshot.family
                            val inviteRole = if (inviteAsManager) 2u.toUByte() else 1u.toUByte()
                            inviteInProgress = true
                            shareStage = context.getString(R.string.invite_preparing)
                            scope.launch {
                                runCatching { withContext(Dispatchers.IO) {
                                    sharing.invite(
                                        invitedFamily, inviteOrigin, inviteRole,
                                    )
                                } }.onSuccess { fragment ->
                                    version++
                                    if (selectedFamily == invitedFamily.familyId.key()) {
                                        invitationFragment = fragment
                                        shareStage = context.getString(R.string.invite_confirmed)
                                        message = null
                                    }
                                }.onFailure {
                                    if (it is kotlinx.coroutines.CancellationException) throw it
                                    if (selectedFamily == invitedFamily.familyId.key()) {
                                        shareStage = context.getString(R.string.share_retry)
                                        message = errorText
                                    }
                                }
                                inviteInProgress = false
                            }
                        }) { Text(stringResource(R.string.create_invite)) }
                        invitationFragment?.let { fragment ->
                            OutlinedButton(onClick = {
                                shareInvitation(context, invitationLink(fragment))
                            }) { Text(stringResource(R.string.share_android_invitation)) }
                            OutlinedButton(onClick = {
                                copyInvitation(context, invitationLink(fragment))
                                message = context.getString(R.string.invitation_copied)
                            }) { Text(stringResource(R.string.copy_android_invitation)) }
                            if (inviteOrigin == PreviewRelay.origin) OutlinedButton(onClick = {
                                shareInvitation(context, browserInvitationLink(inviteOrigin, fragment))
                            }) { Text(stringResource(R.string.share_browser_invitation)) }
                        }
                    }
                }
            }
            if (activeShared && showAccessControls && family != null) OutlinedButton(onClick = {
                scope.launch {
                    runCatching { withContext(Dispatchers.IO) { sharing.privateCopy(family, System.currentTimeMillis()) } }
                        .onSuccess { copy ->
                            selectedFamily = copy.familyId.key()
                            selectedChild = null
                            version++
                            message = context.getString(R.string.private_copy_created)
                        }.onFailure { message = errorText }
                }
            }) { Text(stringResource(R.string.make_private_copy)) }
            if (families.size != 1) {
                Text(stringResource(R.string.families), style = MaterialTheme.typography.titleLarge)
                families.forEachIndexed { index, item ->
                    FilterChip(
                        selected = item.familyId.key() == selectedFamily,
                        onClick = {
                            selectedFamily = item.familyId.key()
                            selectedChild = null
                            showChildDetails = false
                            showAddChildForm = false
                            childName = ""
                            childBirthDate = ""
                            childSex = 3u.toUByte()
                        },
                        label = {
                            val firstChild = familyChildNames[item.familyId.key()]
                            Text(if (firstChild == null) stringResource(R.string.family_number, index + 1)
                                else stringResource(R.string.family_with_child, index + 1, firstChild))
                        },
                    )
                }
            }
            removedFamilies.forEach { source ->
                Card(modifier = Modifier.fillMaxWidth()) {
                    Column(
                        modifier = Modifier.padding(16.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Text(stringResource(R.string.removed_family_card, source.familyId.key().take(8)))
                        OutlinedButton(onClick = {
                            scope.launch {
                                runCatching { withContext(Dispatchers.IO) {
                                    sharing.privateCopy(source, System.currentTimeMillis())
                                } }.onSuccess { copy ->
                                    selectedFamily = copy.familyId.key()
                                    selectedChild = null
                                    version++
                                    message = context.getString(R.string.private_copy_created)
                                }.onFailure { message = errorText }
                            }
                        }) { Text(stringResource(R.string.continue_in_private_copy)) }
                    }
                }
            }
            if (family != null) OutlinedButton(onClick = {
                showFamilySetup = !showFamilySetup
            }) { Text(stringResource(if (showFamilySetup) R.string.hide_family_setup
                else R.string.show_family_setup)) }
            if (family == null || showFamilySetup) OutlinedButton(onClick = {
                scope.launch {
                    runCatching { withContext(Dispatchers.IO) { store.createFamily(System.currentTimeMillis()) } }
                        .onSuccess { created ->
                            selectedFamily = created.familyId.key()
                            selectedChild = null
                            showFamilySetup = false
                            showChildDetails = false
                            showAddChildForm = false
                            childName = ""
                            childBirthDate = ""
                            childSex = 3u.toUByte()
                            version++
                            message = null
                        }.onFailure { message = errorText }
                }
            }) { Text(stringResource(R.string.new_family)) }

            if (!joinFirst && (family == null || showFamilySetup || recipientFamilies.isNotEmpty())) joinControls()

            }
            if (route == TrackerDestination.FAMILY && family != null && activeFamilyIsLocal) {
                if (!activeShared && BuildConfig.DEBUG) Card(modifier = Modifier.fillMaxWidth()) {
                    Column(
                        modifier = Modifier.padding(16.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Text(stringResource(R.string.share_family), style = MaterialTheme.typography.titleMedium)
                        Text(stringResource(R.string.preview_share_description))
                        Button(enabled = !shareInProgress, onClick = {
                            shareInProgress = true
                            shareStage = context.getString(R.string.share_preparing)
                            scope.launch {
                                runCatching { withContext(Dispatchers.IO) {
                                    val cursor = sharing.promote(family, PreviewRelay.origin, PreviewRelay.publicKey)
                                    cursor to recordRelayOrigin(family, PreviewRelay.origin)
                                } }.onSuccess { (cursor, savedOrigin) ->
                                    version++
                                    if (selectedFamily == family.familyId.key()) {
                                        relayOrigin = PreviewRelay.origin
                                        shareStage = if (savedOrigin)
                                            context.getString(R.string.share_confirmed, cursor.toLong())
                                        else context.getString(R.string.share_origin_not_saved)
                                        showAccessControls = true
                                        message = if (savedOrigin) null else errorText
                                    }
                                }.onFailure {
                                    if (it is kotlinx.coroutines.CancellationException) throw it
                                    if (selectedFamily == family.familyId.key()) {
                                        shareStage = context.getString(R.string.share_retry)
                                        message = errorText
                                    }
                                }
                                shareInProgress = false
                            }
                        }) { Text(stringResource(R.string.share_family_action)) }
                        shareStage?.let { Text(it) }
                    }
                }
                if (showFamilySetup && BuildConfig.DEBUG) {
                    if (!showShareForm) OutlinedButton(
                        onClick = { showShareForm = true },
                    ) { Text(stringResource(R.string.sharing_controls)) }
                    else Card(modifier = Modifier.fillMaxWidth()) {
                        Column(
                            modifier = Modifier.padding(16.dp),
                            verticalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            Text(stringResource(R.string.dev_share_title), style = MaterialTheme.typography.titleMedium)
                            Text(stringResource(R.string.dev_share_description))
                            OutlinedTextField(
                                value = relayOrigin,
                                onValueChange = { relayOrigin = it },
                                label = { Text(stringResource(R.string.relay_origin)) },
                                modifier = Modifier.fillMaxWidth(),
                                singleLine = true,
                            )
                            OutlinedTextField(
                                value = relayPublicKey,
                                onValueChange = { relayPublicKey = it },
                                label = { Text(stringResource(R.string.relay_public_key)) },
                                modifier = Modifier.fillMaxWidth(),
                                singleLine = true,
                            )
                            Button(onClick = {
                                shareStage = context.getString(R.string.share_preparing)
                                scope.launch {
                                    runCatching {
                                        withContext(Dispatchers.IO) {
                                            sharing.promote(family, relayOrigin.trim(), relayPublicKey)
                                        }
                                    }.onSuccess { cursor ->
                                        shareStage = context.getString(R.string.share_confirmed, cursor.toLong())
                                        version++
                                        message = if (recordRelayOrigin(family, relayOrigin.trim())) null else errorText
                                    }.onFailure {
                                        shareStage = context.getString(R.string.share_retry)
                                        message = errorText
                                    }
                                }
                            }) { Text(stringResource(R.string.share_retry_button)) }
                            shareStage?.let { Text(it) }
                            OutlinedButton(onClick = {
                                scope.launch {
                                    runCatching {
                                        withContext(Dispatchers.IO) {
                                            sharing.syncAndUpload(family, relayOrigin.trim())
                                        }
                                    }.onSuccess { progress ->
                                        shareStage = sharedSyncMessage(context, progress)
                                        version++
                                        message = null
                                    }.onFailure { message = if (it is SharedUploadBlocked) context.getString(R.string.shared_upload_blocked) else errorText }
                                }
                            }) { Text(stringResource(R.string.sync_shared)) }
                        }
                    }
                }
            }
            if (family != null) {
                if (route == TrackerDestination.FAMILY) {
                restoredOrigin?.let { origin ->
                    Text(stringResource(R.string.restored_from, savedTime(origin.snapshotUtcMs)))
                    if (origin.knownGap) Text(stringResource(R.string.file_known_gap))
                }
                if (children.size != 1) {
                    Text(stringResource(R.string.children), style = MaterialTheme.typography.titleLarge)
                    if (children.isEmpty()) Text(stringResource(R.string.no_children))
                    children.forEach { item ->
                        FilterChip(
                            selected = item.id.key() == selectedChild,
                            onClick = {
                                selectedChild = item.id.key()
                                showChildDetails = false
                                showAddChildForm = false
                                childName = ""
                                childBirthDate = ""
                                childSex = 3u.toUByte()
                            },
                            label = { Text(item.name) },
                        )
                    }
                }
                }
                if (child != null && route == TrackerDestination.TODAY) {
                    Text(stringResource(if (activeShared) R.string.shared_family_short else R.string.local_only),
                        style = MaterialTheme.typography.labelMedium)
                    Text(childAgeLabel(context, child.birthDateString()),
                        style = MaterialTheme.typography.bodyMedium)
                    if (automaticSyncDelayed && !automaticSyncBlocked) Text(
                        stringResource(R.string.automatic_sync_delayed), color = MaterialTheme.colorScheme.error)
                    if (automaticSyncBlocked) Text(stringResource(R.string.shared_upload_blocked),
                        color = MaterialTheme.colorScheme.error)
                    if (loadedChildKey == selectedChild && daySummary != null &&
                        daySummaryDay == LocalDate.now(ZoneId.systemDefault())) {
                        val today = daySummary!!
                        val sleepMinutes = today.sleepMs.toLong() / 60_000L
                        val lastFeed = entries.filter {
                            it.kind == "feed.breast" || it.kind == "feed.bottle" || it.kind == "feed.solids"
                        }
                            .maxByOrNull { it.startUtcMs }
                        val lastDiaper = entries.filter { it.kind == "diaper" }
                            .maxByOrNull { it.startUtcMs }
                        val runningSleep = entries.filter { it.kind == "sleep" && it.endUtcMs == null }
                            .maxByOrNull { it.startUtcMs }
                        Card(Modifier.fillMaxWidth()) {
                            Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                                Text(stringResource(R.string.today_summary), fontWeight = FontWeight.SemiBold)
                                Text(stringResource(R.string.today_sleep, sleepMinutes / 60, sleepMinutes % 60))
                                Text(pluralStringResource(R.plurals.today_feeds,
                                    today.feedCount.toInt(), today.feedCount.toLong(), today.bottleMl.toLong()))
                                Text(pluralStringResource(R.plurals.today_diapers,
                                    today.diaperCount.toInt(), today.diaperCount.toLong(),
                                    today.wetDiaperCount.toLong(), today.dirtyDiaperCount.toLong()))
                                lastFeed?.let { Text(stringResource(R.string.last_feed, savedTime(it.startUtcMs))) }
                                lastDiaper?.let { Text(stringResource(R.string.last_diaper, savedTime(it.startUtcMs))) }
                                if (runningSleep != null) {
                                    val timer = runningSleep
                                    Text(stringResource(R.string.running_sleep_since, savedTime(timer.startUtcMs)))
                                    Button(onClick = {
                                        val end = System.currentTimeMillis()
                                        val endOffset = (TimeZone.getDefault().getOffset(end) / 60_000).toShort()
                                        change {
                                            if (activeShared) sharing.stopSleep(family, timer.childId, timer.id, end, endOffset)
                                            else store.stopSleep(family, timer.childId, timer.id, end, endOffset, end)
                                        }
                                    }) { Text(stringResource(R.string.stop_sleep)) }
                                } else {
                                    Button(onClick = {
                                        change(onSaved = {
                                            if (Build.VERSION.SDK_INT >= 33 &&
                                                context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
                                            ) notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
                                        }) {
                                            if (activeShared) sharing.startSleepWithPlace(family, child.id, nowTime(), null)
                                            else store.startSleepWithPlace(family, child.id, nowTime(), null)
                                        }
                                    }) { Text(stringResource(R.string.start_sleep)) }
                                }
                            }
                        }
                    }
                    Text(stringResource(R.string.quick_log), style = MaterialTheme.typography.titleMedium)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        val description = stringResource(R.string.quick_wet_diaper_description)
                        OutlinedButton(onClick = {
                            change {
                                val at = nowTime()
                                if (activeShared) sharing.logDiaper(family, child.id, 1u.toUByte(), at)
                                else store.logDiaper(family, child.id, 1u.toUByte(), at)
                            }
                        }, modifier = Modifier.weight(1f).semantics { contentDescription = description }) {
                            Text(stringResource(R.string.quick_wet_diaper))
                        }
                        OutlinedButton(onClick = {
                            captureKind = CaptureKind.BOTTLE
                            destination = TrackerDestination.CAPTURE
                        }, modifier = Modifier.weight(1f)) { Text(stringResource(R.string.event_bottle)) }
                    }
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedButton(onClick = {
                            captureKind = CaptureKind.DIAPER
                            destination = TrackerDestination.CAPTURE
                        }, modifier = Modifier.weight(1f)) { Text(stringResource(R.string.event_diaper)) }
                        Button(onClick = {
                            captureKind = null
                            destination = TrackerDestination.CAPTURE
                        }, modifier = Modifier.weight(1f)) {
                            Text(stringResource(R.string.add_activity))
                        }
                    }
                    Text(stringResource(R.string.recent_entries), style = MaterialTheme.typography.titleMedium)
                    val recentEntries = if (loadedChildKey == selectedChild)
                        entries.sortedByDescending { it.startUtcMs }.take(3) else emptyList()
                    if (recentEntries.isEmpty()) Text(stringResource(R.string.no_entries))
                    recentEntries.forEach { entry ->
                        Card(Modifier.fillMaxWidth()) {
                            Column(Modifier.padding(12.dp)) {
                                Text(stringResource(activityLabel(entry.kind)), fontWeight = FontWeight.SemiBold)
                                Text(savedTime(entry.startUtcMs), style = MaterialTheme.typography.bodySmall)
                            }
                        }
                    }
                    TextButton(onClick = { destination = TrackerDestination.HISTORY }) {
                        Text(stringResource(R.string.view_timeline))
                    }
                }
                if (child != null && route == TrackerDestination.FAMILY) {
                    Text(childAgeLabel(context, child.birthDateString()),
                        style = MaterialTheme.typography.bodyMedium)
                    OutlinedButton(onClick = { showChildDetails = !showChildDetails }) {
                        Text(stringResource(if (showChildDetails) R.string.hide_child_options
                            else R.string.child_options))
                    }
                    if (showChildDetails) OutlinedButton(onClick = {
                        val birthDate = child.birthDateString()
                        val sex = child.sex ?: 3u.toUByte()
                        pendingChildProfileEdit = PendingChildProfileEdit(
                            family, child.id.copyOf(), activeShared, child.name, child.name,
                            birthDate, birthDate, sex, sex,
                        )
                    }) { Text(stringResource(R.string.edit_child_profile)) }
                }
                if (route == TrackerDestination.FAMILY && children.isNotEmpty() && showChildDetails) OutlinedButton(onClick = {
                    showAddChildForm = true
                    childName = ""
                    childBirthDate = ""
                    childSex = 3u.toUByte()
                }) { Text(stringResource(R.string.add_another_child)) }
                if (route == TrackerDestination.FAMILY && children.isEmpty()) Button(
                    onClick = { showAddChildForm = true },
                ) { Text(stringResource(R.string.add_child)) }

                if (child != null && route == TrackerDestination.CAPTURE) {
                    if (captureKind == null) {
                        Text(stringResource(R.string.add_activity), style = MaterialTheme.typography.titleLarge)
                        CaptureKind.entries.chunked(2).forEach { kinds ->
                            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                kinds.forEach { kind ->
                                    OutlinedButton(
                                        onClick = {
                                            captureKind = kind
                                            scope.launch { captureScrollState.scrollTo(0) }
                                        },
                                        modifier = Modifier.weight(1f).heightIn(min = 88.dp),
                                    ) { Text(stringResource(kind.label), textAlign = TextAlign.Center) }
                                }
                            }
                        }
                    } else {
                        Text(stringResource(R.string.capture_for_child, child.name),
                            style = MaterialTheme.typography.titleMedium)
                    Text(stringResource(R.string.log_time_title), style = MaterialTheme.typography.titleMedium)
                    Text(if (logAtMs == null) stringResource(R.string.log_time_now)
                        else stringResource(R.string.log_time_selected,
                            DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(logAtMs!!))))
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedButton(onClick = {
                            val current = java.util.Calendar.getInstance()
                            DatePickerDialog(context, { _, year, month, day ->
                                TimePickerDialog(context, { _, hour, minute ->
                                    val selected = LocalDateTime.of(year, month + 1, day, hour, minute)
                                        .atZone(ZoneId.systemDefault()).toInstant().toEpochMilli()
                                    if (selected > System.currentTimeMillis()) {
                                        message = context.getString(R.string.log_time_future)
                                    } else {
                                        logAtMs = selected
                                        message = null
                                    }
                                }, current.get(java.util.Calendar.HOUR_OF_DAY),
                                    current.get(java.util.Calendar.MINUTE), false).show()
                            }, current.get(java.util.Calendar.YEAR),
                                current.get(java.util.Calendar.MONTH),
                                current.get(java.util.Calendar.DAY_OF_MONTH)).apply {
                                datePicker.maxDate = System.currentTimeMillis()
                            }.show()
                        }) { Text(stringResource(R.string.log_time_choose)) }
                        if (logAtMs != null) OutlinedButton(onClick = { logAtMs = null }) {
                            Text(stringResource(R.string.log_time_reset))
                        }
                    }
                    if (captureKind == CaptureKind.DIAPER) {
                    Text(stringResource(R.string.log_diaper), style = MaterialTheme.typography.titleLarge)
                    listOf(
                        1u.toUByte() to R.string.wet,
                        2u.toUByte() to R.string.dirty,
                        3u.toUByte() to R.string.both,
                        4u.toUByte() to R.string.dry,
                    ).chunked(2).forEach { options ->
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            options.forEach { (kind, label) ->
                                Button(onClick = { logCompleted { at ->
                                    if (activeShared) sharing.logDiaper(family, child.id, kind, at)
                                    else store.logDiaper(family, child.id, kind, at)
                                } }) {
                                    Text(stringResource(label))
                                }
                            }
                        }
                    }
                    }
                    if (captureKind == CaptureKind.BOTTLE) {
                    Text(stringResource(R.string.log_bottle), style = MaterialTheme.typography.titleLarge)
                    listOf(
                        1u.toUByte() to R.string.bottle_breast_milk,
                        2u.toUByte() to R.string.bottle_formula,
                        3u.toUByte() to R.string.bottle_mixed,
                        4u.toUByte() to R.string.bottle_other,
                    ).chunked(2).forEach { options ->
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            options.forEach { (content, label) ->
                                FilterChip(selected = bottleContent == content,
                                    onClick = { bottleContent = content }, label = { Text(stringResource(label)) })
                            }
                        }
                    }
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        listOf(
                            1u.toUByte() to R.string.unit_ml,
                            2u.toUByte() to R.string.unit_us_fl_oz,
                            3u.toUByte() to R.string.unit_uk_fl_oz,
                        ).forEach { (unit, label) ->
                            FilterChip(selected = bottleUnit == unit, onClick = {
                                if (bottleUnit != unit) {
                                    bottleUnit = unit
                                    amount = ""
                                }
                            }, label = { Text(stringResource(label)) })
                        }
                    }
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedTextField(
                            value = amount,
                            onValueChange = { amount = decimalDraft(it, bottleUnit != 1u.toUByte()) },
                            label = { Text(stringResource(R.string.bottle_amount)) },
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                            modifier = Modifier.weight(1f),
                            singleLine = true,
                        )
                        Button(enabled = validBottleAmount(amount, bottleUnit), onClick = {
                            val entered = amount
                            val unit = bottleUnit
                            val content = bottleContent
                            logCompleted(onSaved = { if (amount == entered) amount = "" }) { at ->
                                if (activeShared) sharing.logBottleEntered(family, child.id, canonicalDecimal(entered), unit, content, at)
                                else store.logBottleEntered(family, child.id, canonicalDecimal(entered), unit, content, at)
                            }
                        }) { Text(stringResource(R.string.log_bottle)) }
                    }
                    }
                    if (captureKind == CaptureKind.BREAST) {
                    Text(stringResource(R.string.log_breast), style = MaterialTheme.typography.titleLarge)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        listOf(1u.toUByte() to R.string.breast_left, 2u.toUByte() to R.string.breast_right).forEach { (side, label) ->
                            FilterChip(selected = breastSide == side, onClick = { breastSide = side },
                                label = { Text(stringResource(label)) })
                        }
                    }
                    OutlinedTextField(
                        value = breastMinutes,
                        onValueChange = { breastMinutes = it.filter(Char::isDigit).take(3) },
                        label = { Text(stringResource(R.string.breast_minutes)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                        singleLine = true,
                    )
                    if (breastDraftSegments.isNotEmpty()) {
                        Text(stringResource(R.string.breast_segments_draft,
                            breastDraftSegments.joinToString(" → ") { (side, minutes) ->
                                context.getString(R.string.breast_segment_summary,
                                    context.getString(if (side == 1u.toUByte()) R.string.breast_left else R.string.breast_right),
                                    minutes)
                            }))
                        OutlinedButton(onClick = { breastDraftSegments = breastDraftSegments.dropLast(1) }) {
                            Text(stringResource(R.string.remove_last_segment))
                        }
                    }
                    val breastNextMinutes = breastMinutes.toLongOrNull()
                    val breastTotalMinutes = breastDraftSegments.sumOf { it.second } + (breastNextMinutes ?: 0L)
                    OutlinedButton(
                        enabled = breastNextMinutes != null && breastNextMinutes in 1L..240L &&
                            breastDraftSegments.size < 7 && breastTotalMinutes <= 240L,
                        onClick = {
                            val minutes = breastMinutes.toLongOrNull() ?: return@OutlinedButton
                            breastDraftSegments = breastDraftSegments + (breastSide to minutes)
                            breastMinutes = ""
                        },
                    ) { Text(stringResource(R.string.add_breast_segment)) }
                    Button(enabled = breastNextMinutes != null && breastNextMinutes in 1L..240L &&
                        breastDraftSegments.size < 8 && breastTotalMinutes <= 240L, onClick = {
                        val minutes = breastMinutes.toLongOrNull() ?: return@Button
                        val draft = breastDraftSegments
                        val plan = draft + (breastSide to minutes)
                        val chosenAt = logAtMs
                        val end = logTime()
                        var cursor = end.startUtcMs - plan.sumOf { it.second * 60_000L }
                        val zone = TimeZone.getDefault()
                        val interval = ActivityWhen(cursor, (zone.getOffset(cursor) / 60_000).toShort(), end.savedAtMs)
                        val segments = plan.map { (side, duration) ->
                            val next = cursor + duration * 60_000L
                            BreastSegmentRow(side, cursor, next,
                                (zone.getOffset(cursor) / 60_000).toShort(),
                                (zone.getOffset(next) / 60_000).toShort()).also { cursor = next }
                        }
                        scope.launch {
                            runCatching { withContext(Dispatchers.IO) {
                                if (activeShared) sharing.logBreastFeedSegments(family, child.id, segments, interval)
                                else store.logBreastFeedSegments(family, child.id, segments, interval)
                            } }.onSuccess {
                                if (breastMinutes.toLongOrNull() == minutes && breastDraftSegments == draft) {
                                    breastMinutes = ""
                                    breastDraftSegments = emptyList()
                                }
                                version++
                                message = null
                                resetLogTime(chosenAt)
                                finishCapture()
                            }.onFailure { message = errorText }
                        }
                    }) { Text(stringResource(R.string.save_breast)) }
                    }
                    if (captureKind == CaptureKind.PUMP) {
                    Text(stringResource(R.string.log_pump), style = MaterialTheme.typography.titleLarge)
                    OutlinedTextField(
                        value = pumpMinutes,
                        onValueChange = { pumpMinutes = it.filter(Char::isDigit).take(3) },
                        label = { Text(stringResource(R.string.pump_minutes)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                        singleLine = true,
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedTextField(
                            value = pumpLeft,
                            onValueChange = { pumpLeft = it.filter(Char::isDigit).take(6) },
                            label = { Text(stringResource(R.string.pump_left_ml)) },
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                            modifier = Modifier.weight(1f),
                            singleLine = true,
                        )
                        OutlinedTextField(
                            value = pumpRight,
                            onValueChange = { pumpRight = it.filter(Char::isDigit).take(6) },
                            label = { Text(stringResource(R.string.pump_right_ml)) },
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                            modifier = Modifier.weight(1f),
                            singleLine = true,
                        )
                    }
                    OutlinedTextField(
                        value = pumpTotal,
                        onValueChange = { pumpTotal = it.filter(Char::isDigit).take(6) },
                        label = { Text(stringResource(R.string.pump_total_ml)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                        singleLine = true,
                    )
                    val pumpDuration = pumpMinutes.toLongOrNull() ?: 0L
                    val pumpSides = (pumpLeft.toLongOrNull() ?: 0L) + (pumpRight.toLongOrNull() ?: 0L)
                    val pumpCanSave = pumpDuration in 1L..240L &&
                        ((pumpTotal.isBlank() && pumpSides > 0L) ||
                            (pumpLeft.isBlank() && pumpRight.isBlank() &&
                                (pumpTotal.toLongOrNull() ?: 0L) > 0L))
                    Button(enabled = pumpCanSave, onClick = {
                        val enteredMinutes = pumpMinutes
                        val enteredLeft = pumpLeft
                        val enteredRight = pumpRight
                        val enteredTotal = pumpTotal
                        val minutes = pumpMinutes.toLongOrNull() ?: return@Button
                        val left = pumpLeft.toLongOrNull()
                        val right = pumpRight.toLongOrNull()
                        val total = pumpTotal.toLongOrNull()
                        val input = PumpInput(left, right, total)
                        val chosenAt = logAtMs
                        val end = logTime()
                        val interval = ActivityWhen(end.startUtcMs - minutes * 60_000L,
                            (TimeZone.getDefault().getOffset(end.startUtcMs - minutes * 60_000L) / 60_000).toShort(), end.savedAtMs)
                        scope.launch {
                            runCatching { withContext(Dispatchers.IO) {
                                if (activeShared) sharing.logPump(family, child.id, input, interval, end.startUtcMs)
                                else store.logPump(family, child.id, input, interval, end.startUtcMs)
                            } }.onSuccess {
                                if (pumpMinutes == enteredMinutes) pumpMinutes = ""
                                if (pumpLeft == enteredLeft) pumpLeft = ""
                                if (pumpRight == enteredRight) pumpRight = ""
                                if (pumpTotal == enteredTotal) pumpTotal = ""
                                version++
                                message = null
                                resetLogTime(chosenAt)
                                finishCapture()
                            }.onFailure { message = errorText }
                        }
                    }) { Text(stringResource(R.string.save_pump)) }
                    }
                    if (captureKind == CaptureKind.SOLIDS) {
                    Text(stringResource(R.string.log_solids), style = MaterialTheme.typography.titleLarge)
                    OutlinedTextField(
                        value = solidsFoods,
                        onValueChange = { solidsFoods = it.take(2048) },
                        label = { Text(stringResource(R.string.solids_foods)) },
                        modifier = Modifier.fillMaxWidth(),
                    )
                    OutlinedTextField(
                        value = solidsAmount,
                        onValueChange = { solidsAmount = it.take(256) },
                        label = { Text(stringResource(R.string.solids_amount)) },
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Button(enabled = solidsFoods.isNotBlank(), onClick = {
                        val enteredFoods = solidsFoods
                        val enteredAmount = solidsAmount
                        val foods = solidsFoods.lines().map { it.trim() }.filter { it.isNotEmpty() }
                        val eaten = solidsAmount.trim()
                        val chosenAt = logAtMs
                        val at = logTime()
                        scope.launch {
                            runCatching { withContext(Dispatchers.IO) {
                                if (activeShared) sharing.logSolids(family, child.id, foods, eaten, at)
                                else store.logSolids(family, child.id, foods, eaten, at)
                            } }.onSuccess {
                                if (solidsFoods == enteredFoods) solidsFoods = ""
                                if (solidsAmount == enteredAmount) solidsAmount = ""
                                version++
                                message = null
                                resetLogTime(chosenAt)
                                finishCapture()
                            }.onFailure { message = errorText }
                        }
                    }) { Text(stringResource(R.string.save_solids)) }
                    }
                    if (captureKind == CaptureKind.SLEEP) {
                    Text(stringResource(R.string.log_sleep), style = MaterialTheme.typography.titleLarge)
                    Text(stringResource(R.string.sleep_place_title))
                    listOf(
                        null to R.string.sleep_place_unspecified,
                        1u.toUByte() to R.string.sleep_place_crib,
                        2u.toUByte() to R.string.sleep_place_pram,
                        3u.toUByte() to R.string.sleep_place_contact,
                        4u.toUByte() to R.string.sleep_place_car,
                        5u.toUByte() to R.string.sleep_place_other,
                    ).chunked(2).forEach { options ->
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            options.forEach { (place, label) ->
                                FilterChip(
                                    selected = sleepPlace == place,
                                    onClick = { sleepPlace = place },
                                    label = { Text(stringResource(label)) },
                                )
                            }
                        }
                    }
                    Button(onClick = {
                        val enteredPlace = sleepPlace
                        change(onSaved = {
                            if (sleepPlace == enteredPlace) sleepPlace = null
                            if (Build.VERSION.SDK_INT >= 33 &&
                                context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
                            ) notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
                            finishCapture()
                        }) {
                            if (activeShared) sharing.startSleepWithPlace(family, child.id, nowTime(), enteredPlace)
                            else store.startSleepWithPlace(family, child.id, nowTime(), enteredPlace)
                        }
                    }) { Text(stringResource(R.string.start_sleep)) }
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedTextField(
                            value = sleepMinutes,
                            onValueChange = { sleepMinutes = it.filter(Char::isDigit) },
                            label = { Text(stringResource(R.string.sleep_minutes)) },
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                            modifier = Modifier.weight(1f),
                            singleLine = true,
                        )
                        Button(enabled = (sleepMinutes.toLongOrNull() ?: 0L) in 1L..1440L, onClick = {
                            val enteredMinutes = sleepMinutes
                            val enteredPlace = sleepPlace
                            val duration = sleepMinutes.toLongOrNull() ?: return@Button
                            val chosenAt = logAtMs
                            val end = logTime().startUtcMs
                            val start = end - duration * 60_000
                            val zone = TimeZone.getDefault()
                            val whenStarted = ActivityWhen(start, (zone.getOffset(start) / 60_000).toShort(), System.currentTimeMillis())
                            val endOffset = (zone.getOffset(end) / 60_000).toShort()
                            change(onSaved = {
                                resetLogTime(chosenAt)
                                finishCapture()
                                if (sleepMinutes == enteredMinutes) sleepMinutes = ""
                                if (sleepPlace == enteredPlace) sleepPlace = null
                            }) {
                                if (activeShared) sharing.logSleepWithPlace(family, child.id, whenStarted, end, endOffset, enteredPlace)
                                else store.logSleepWithPlace(family, child.id, whenStarted, end, endOffset, enteredPlace)
                            }
                        }) { Text(stringResource(R.string.save_sleep)) }
                    }
                    }
                    if (captureKind == CaptureKind.GROWTH) {
                    Text(stringResource(R.string.log_growth), style = MaterialTheme.typography.titleLarge)
                    GrowthUnitChoices(R.string.weight_unit, massUnits, growthWeightUnit) {
                        if (growthWeightUnit != it) { growthWeightUnit = it; growthWeight = "" }
                    }
                    OutlinedTextField(
                        value = growthWeight,
                        onValueChange = { growthWeight = decimalDraft(it, growthWeightUnit != 10u.toUByte()) },
                        label = { Text(stringResource(R.string.weight)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                    )
                    GrowthUnitChoices(R.string.length_unit, lengthUnits, growthLengthUnit) {
                        if (growthLengthUnit != it) { growthLengthUnit = it; growthLength = "" }
                    }
                    OutlinedTextField(
                        value = growthLength,
                        onValueChange = { growthLength = decimalDraft(it, growthLengthUnit != 20u.toUByte()) },
                        label = { Text(stringResource(R.string.length)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                    )
                    GrowthUnitChoices(R.string.head_unit, lengthUnits, growthHeadUnit) {
                        if (growthHeadUnit != it) { growthHeadUnit = it; growthHead = "" }
                    }
                    OutlinedTextField(
                        value = growthHead,
                        onValueChange = { growthHead = decimalDraft(it, growthHeadUnit != 20u.toUByte()) },
                        label = { Text(stringResource(R.string.head_circumference)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                    )
                    Button(
                        enabled = listOf(growthWeight, growthLength, growthHead).any { it.isNotBlank() } &&
                            validGrowthAmount(growthWeight, growthWeightUnit) &&
                            validGrowthAmount(growthLength, growthLengthUnit) &&
                            validGrowthAmount(growthHead, growthHeadUnit),
                        onClick = {
                            val savedWeight = growthWeight
                            val savedLength = growthLength
                            val savedHead = growthHead
                            val input = growthInput(savedWeight, growthWeightUnit,
                                savedLength, growthLengthUnit, savedHead, growthHeadUnit)
                            val chosenAt = logAtMs
                            val at = logTime()
                            scope.launch {
                                runCatching { withContext(Dispatchers.IO) {
                                    if (activeShared) sharing.logGrowthEntered(family, child.id, input, at)
                                    else store.logGrowthEntered(family, child.id, input, at)
                                } }.onSuccess {
                                    if (growthWeight == savedWeight) growthWeight = ""
                                    if (growthLength == savedLength) growthLength = ""
                                    if (growthHead == savedHead) growthHead = ""
                                    version++
                                    message = null
                                    resetLogTime(chosenAt)
                                    finishCapture()
                                }.onFailure { message = errorText }
                            }
                        },
                    ) { Text(stringResource(R.string.save_growth)) }
                    }
                    if (captureKind == CaptureKind.TEMPERATURE) {
                    Text(stringResource(R.string.log_temperature), style = MaterialTheme.typography.titleLarge)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        listOf(30u.toUByte() to R.string.unit_celsius,
                            31u.toUByte() to R.string.unit_fahrenheit).forEach { (unit, label) ->
                            FilterChip(selected = temperatureUnit == unit, onClick = {
                                if (temperatureUnit != unit) {
                                    temperatureUnit = unit
                                    temperatureEntered = ""
                                }
                            }, label = { Text(stringResource(label)) })
                        }
                    }
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedTextField(
                            value = temperatureEntered,
                            onValueChange = { temperatureEntered = decimalDraft(it, fractional = true, signed = true) },
                            label = { Text(stringResource(if (temperatureUnit == 30u.toUByte())
                                R.string.temperature_c else R.string.temperature_f)) },
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                            modifier = Modifier.weight(1f),
                            singleLine = true,
                        )
                        Button(enabled = validTemperature(temperatureEntered), onClick = {
                            val entered = temperatureEntered.trim()
                            val unit = temperatureUnit
                            val chosenAt = logAtMs
                            val at = logTime()
                            scope.launch {
                                runCatching { withContext(Dispatchers.IO) {
                                    if (activeShared) sharing.logTemperatureEntered(family, child.id, canonicalDecimal(entered), unit, at)
                                    else store.logTemperatureEntered(family, child.id, canonicalDecimal(entered), unit, at)
                                } }.onSuccess {
                                    if (temperatureEntered.trim() == entered) temperatureEntered = ""
                                    version++
                                    message = null
                                    resetLogTime(chosenAt)
                                    finishCapture()
                                }.onFailure { message = errorText }
                            }
                        }) { Text(stringResource(R.string.save_temperature)) }
                    }
                    }
                    if (captureKind == CaptureKind.MEDICATION) {
                    Text(stringResource(R.string.log_medication), style = MaterialTheme.typography.titleLarge)
                    OutlinedTextField(
                        value = medicationName,
                        onValueChange = { medicationName = it.take(256) },
                        label = { Text(stringResource(R.string.medication_name)) },
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedTextField(
                            value = doseAmount,
                            onValueChange = { doseAmount = it.take(64) },
                            label = { Text(stringResource(R.string.dose_amount)) },
                            modifier = Modifier.weight(1f),
                            singleLine = true,
                        )
                        OutlinedTextField(
                            value = doseUnit,
                            onValueChange = { doseUnit = it.take(64) },
                            label = { Text(stringResource(R.string.dose_unit)) },
                            modifier = Modifier.weight(1f),
                            singleLine = true,
                        )
                    }
                    Button(
                        enabled = medicationName.isNotBlank() && doseAmount.isNotBlank() && doseUnit.isNotBlank(),
                        onClick = {
                            val name = medicationName.trim()
                            val amount = doseAmount.trim()
                            val unit = doseUnit.trim()
                            val input = MedicationInput(name, amount, unit)
                            val chosenAt = logAtMs
                            val at = logTime()
                            scope.launch {
                                runCatching { withContext(Dispatchers.IO) {
                                    if (activeShared) sharing.logMedication(family, child.id, input, at)
                                    else store.logMedication(family, child.id, input, at)
                                } }.onSuccess {
                                    if (medicationName.trim() == name) medicationName = ""
                                    if (doseAmount.trim() == amount) doseAmount = ""
                                    if (doseUnit.trim() == unit) doseUnit = ""
                                    version++
                                    message = null
                                    resetLogTime(chosenAt)
                                    finishCapture()
                                }.onFailure { message = errorText }
                            }
                        },
                    ) { Text(stringResource(R.string.save_medication)) }
                    }
                    if (captureKind == CaptureKind.NOTE) {
                    Text(stringResource(R.string.log_note), style = MaterialTheme.typography.titleLarge)
                    OutlinedTextField(
                        value = noteText,
                        onValueChange = { noteText = it.take(4096) },
                        label = { Text(stringResource(R.string.note_text)) },
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Button(enabled = noteText.isNotBlank(), onClick = {
                        val note = noteText.trim()
                        val chosenAt = logAtMs
                        val at = logTime()
                        scope.launch {
                            runCatching { withContext(Dispatchers.IO) {
                                if (activeShared) sharing.logNote(family, child.id, note, at)
                                else store.logNote(family, child.id, note, at)
                            } }.onSuccess {
                                if (noteText.trim() == note) noteText = ""
                                version++
                                message = null
                                resetLogTime(chosenAt)
                                finishCapture()
                            }.onFailure { message = errorText }
                        }
                    }) { Text(stringResource(R.string.save_note)) }
                    }
                    }
                }
                if (child != null && route == TrackerDestination.HISTORY) {
                    Text(stringResource(R.string.timeline), style = MaterialTheme.typography.titleLarge)
                    OutlinedButton(onClick = {
                        val day = selectedHistoryDay?.let { LocalDate.parse(it) } ?: LocalDate.now()
                        DatePickerDialog(context, { _, year, month, date ->
                            selectedHistoryDay = LocalDate.of(year, month + 1, date).toString()
                            expandedEntryKey = null
                        }, day.year, day.monthValue - 1, day.dayOfMonth).apply {
                            datePicker.maxDate = System.currentTimeMillis()
                        }.show()
                    }) {
                        Text(selectedHistoryDay?.let { iso ->
                            val day = LocalDate.parse(iso)
                            DateFormat.getDateInstance(DateFormat.MEDIUM).format(
                                Date.from(day.atStartOfDay(ZoneId.systemDefault()).toInstant()))
                        } ?: stringResource(R.string.choose_history_day))
                    }
                    if (selectedHistoryDay != null) TextButton(onClick = {
                        selectedHistoryDay = null
                        expandedEntryKey = null
                    }) { Text(stringResource(R.string.show_all_days)) }
                    listOf(
                        TimelineFilter.ALL to R.string.timeline_all,
                        TimelineFilter.FEEDS to R.string.timeline_feeds,
                        TimelineFilter.SLEEP to R.string.timeline_sleep,
                        TimelineFilter.DIAPERS to R.string.timeline_diapers,
                        TimelineFilter.CARE to R.string.timeline_care,
                        TimelineFilter.NOTES to R.string.timeline_notes,
                    ).chunked(2).forEach { options ->
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            options.forEach { (filter, label) ->
                                FilterChip(selected = timelineFilter == filter,
                                    onClick = { timelineFilter = filter },
                                    label = { Text(stringResource(label)) })
                            }
                        }
                    }
                    val currentEntries = if (loadedChildKey == selectedChild) entries else emptyList()
                    val visibleEntries = currentEntries.filter { entry ->
                        timelineFilter.includes(entry.kind) &&
                            (selectedHistoryDay == null || Instant.ofEpochMilli(entry.startUtcMs)
                                .atZone(ZoneId.systemDefault()).toLocalDate().toString() == selectedHistoryDay)
                    }
                    if (visibleEntries.isEmpty()) Text(stringResource(
                        if (currentEntries.isEmpty()) R.string.no_entries else R.string.no_matching_entries))
                    var previousDay: LocalDate? = null
                    visibleEntries.forEach { entry ->
                        val day = Instant.ofEpochMilli(entry.startUtcMs)
                            .atZone(ZoneId.systemDefault()).toLocalDate()
                        if (day != previousDay) {
                            Text(DateFormat.getDateInstance(DateFormat.FULL).format(Date(entry.startUtcMs)),
                                style = MaterialTheme.typography.titleMedium)
                            previousDay = day
                        }
                        val label = when {
                            entry.bottleMl != null -> stringResource(R.string.bottle_with_entered,
                                localizedEntered(context, entry.bottleEntered ?: entry.bottleMl.toString()),
                                stringResource(when (entry.bottleUnit) {
                                    2u.toUByte() -> R.string.unit_us_fl_oz
                                    3u.toUByte() -> R.string.unit_uk_fl_oz
                                    else -> R.string.unit_ml
                                }),
                                stringResource(when (entry.bottleContent) {
                                    1u.toUByte() -> R.string.bottle_breast_milk
                                    2u.toUByte() -> R.string.bottle_formula
                                    3u.toUByte() -> R.string.bottle_mixed
                                    else -> R.string.bottle_other
                                }))
                            entry.kind == "feed.breast" && entry.breastSide != null && entry.endUtcMs != null ->
                                stringResource(R.string.breast_entry,
                                    stringResource(if (entry.breastSide == 1u.toUByte()) R.string.breast_left else R.string.breast_right),
                                    (entry.endUtcMs!! - entry.startUtcMs) / 60_000L)
                            entry.kind == "feed.breast" && entry.breastSegments != null ->
                                stringResource(R.string.breast_multi_entry,
                                    entry.breastSegments!!.joinToString(" → ") { segment ->
                                        context.getString(R.string.breast_segment_summary,
                                            context.getString(if (segment.side == 1u.toUByte()) R.string.breast_left else R.string.breast_right),
                                            (segment.endUtcMs - segment.startUtcMs) / 60_000L)
                                    })
                            entry.kind == "pump" && entry.pumpTotalMl != null && entry.endUtcMs != null ->
                                stringResource(R.string.pump_total_entry, entry.pumpTotalMl!!,
                                    (entry.endUtcMs!! - entry.startUtcMs) / 60_000L)
                            entry.kind == "pump" && entry.endUtcMs != null &&
                                (entry.pumpLeftMl != null || entry.pumpRightMl != null) ->
                                stringResource(R.string.pump_sides_entry, entry.pumpLeftMl ?: 0L,
                                    entry.pumpRightMl ?: 0L, (entry.endUtcMs!! - entry.startUtcMs) / 60_000L)
                            entry.kind == "feed.solids" && entry.solidsFoods != null -> {
                                val foods = entry.solidsFoods!!.joinToString(", ")
                                if (entry.solidsAmount.isNullOrBlank()) stringResource(R.string.solids_entry, foods)
                                else stringResource(R.string.solids_entry_amount, foods, entry.solidsAmount!!)
                            }
                            entry.kind == "sleep" && entry.endUtcMs != null ->
                                stringResource(R.string.sleep_duration, (entry.endUtcMs!! - entry.startUtcMs) / 60_000)
                            entry.kind == "sleep" -> stringResource(R.string.sleep_running)
                            entry.kind == "note" && entry.note != null -> stringResource(R.string.note_entry, entry.note!!)
                            entry.kind == "growth" -> {
                                val parts = listOfNotNull(
                                    growthDisplay(context, entry.growthWeightG, entry.growthWeightEntered,
                                        entry.growthWeightUnit, 10u.toUByte()),
                                    growthDisplay(context, entry.growthLengthMm, entry.growthLengthEntered,
                                        entry.growthLengthUnit, 20u.toUByte()),
                                    growthDisplay(context, entry.growthHeadMm, entry.growthHeadEntered,
                                        entry.growthHeadUnit, 20u.toUByte())?.let {
                                        context.getString(R.string.growth_head_part, it)
                                    },
                                )
                                if (parts.isEmpty()) entry.kind
                                else stringResource(R.string.growth_summary, parts.joinToString(" · "))
                            }
                            entry.kind == "temperature" && entry.temperatureC != null ->
                                stringResource(R.string.temperature_entry,
                                    localizedEntered(context, entry.temperatureEntered ?: entry.temperatureC!!),
                                    stringResource(if (entry.temperatureUnit == 31u.toUByte())
                                        R.string.unit_fahrenheit else R.string.unit_celsius))
                            entry.kind == "medication" && entry.medicationName != null &&
                                entry.medicationDoseAmount != null && entry.medicationDoseUnit != null ->
                                stringResource(
                                    R.string.medication_entry, entry.medicationName!!,
                                    entry.medicationDoseAmount!!, entry.medicationDoseUnit!!,
                                )
                            entry.diaperKind != null -> stringResource(R.string.diaper, when (entry.diaperKind!!.toInt()) {
                                1 -> stringResource(R.string.wet)
                                2 -> stringResource(R.string.dirty)
                                3 -> stringResource(R.string.both)
                                else -> stringResource(R.string.dry)
                            })
                            else -> stringResource(R.string.unknown_activity)
                        }
                        Card(Modifier.fillMaxWidth()) {
                            Column(Modifier.padding(12.dp)) {
                                Text(label, fontWeight = FontWeight.SemiBold)
                                Text(DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(entry.startUtcMs)))
                                if (entry.kind != "note" && entry.note != null) {
                                    Text(stringResource(R.string.activity_note, entry.note!!))
                                }
                                if (entry.kind == "sleep" && entry.sleepPlace != null) {
                                    val placeLabel = when (entry.sleepPlace!!.toInt()) {
                                        1 -> R.string.sleep_place_crib
                                        2 -> R.string.sleep_place_pram
                                        3 -> R.string.sleep_place_contact
                                        4 -> R.string.sleep_place_car
                                        else -> R.string.sleep_place_other
                                    }
                                    Text(stringResource(R.string.sleep_place_entry, stringResource(placeLabel)))
                                }
                                val entryKey = entry.id.key()
                                TextButton(onClick = {
                                    expandedEntryKey = if (expandedEntryKey == entryKey) null else entryKey
                                }) {
                                    Text(stringResource(if (expandedEntryKey == entryKey)
                                        R.string.hide_entry_actions else R.string.show_entry_actions))
                                }
                                if (expandedEntryKey == entryKey) {
                                if (entry.kind == "sleep" && entry.endUtcMs == null) {
                                    Button(onClick = {
                                        val end = System.currentTimeMillis()
                                        val endOffset = (TimeZone.getDefault().getOffset(end) / 60_000).toShort()
                                        change {
                                            if (activeShared) sharing.stopSleep(family, entry.childId, entry.id, end, endOffset)
                                            else store.stopSleep(family, entry.childId, entry.id, end, endOffset, end)
                                        }
                                    }) { Text(stringResource(R.string.stop_sleep)) }
                                }
                                if (entry.kind == "sleep" && entry.endUtcMs != null) {
                                    OutlinedButton(onClick = {
                                        pendingSleepEdit = PendingSleepEdit(
                                            family, entry.childId.copyOf(), entry.id.copyOf(), activeShared,
                                            entry.startUtcMs,
                                            ((entry.endUtcMs!! - entry.startUtcMs) / 60_000L).toString(),
                                        )
                                    }) { Text(stringResource(R.string.edit_sleep)) }
                                }
                                if (entry.kind == "sleep") {
                                    OutlinedButton(onClick = {
                                        pendingSleepPlaceEdit = PendingSleepPlaceEdit(
                                            family, entry.childId.copyOf(), entry.id.copyOf(),
                                            activeShared, entry.sleepPlace,
                                        )
                                    }) { Text(stringResource(R.string.edit_sleep_place)) }
                                }
                                if (entry.kind in noteEditableKinds) {
                                    OutlinedButton(onClick = {
                                        pendingNoteEdit = PendingNoteEdit(
                                            family,
                                            entry.childId.copyOf(),
                                            entry.id.copyOf(),
                                            activeShared,
                                            entry.note.orEmpty(),
                                            entry.kind == "note",
                                            entry.note != null,
                                        )
                                    }) { Text(stringResource(if (entry.note == null)
                                        R.string.add_activity_note else R.string.edit_note)) }
                                }
                                if (entry.kind in instantTimeEditableKinds) {
                                    OutlinedButton(onClick = {
                                        pendingTimeEdit = PendingTimeEdit(
                                            family, entry.childId.copyOf(), entry.id.copyOf(),
                                            activeShared, entry.startUtcMs, entry.offsetMinutes,
                                        )
                                    }) { Text(stringResource(R.string.edit_entry_time)) }
                                }
                                if ((entry.kind == "sleep" || entry.kind == "pump") &&
                                    entry.endUtcMs != null && entry.endUtcMs!! > entry.startUtcMs) {
                                    OutlinedButton(onClick = {
                                        pendingTimeEdit = PendingTimeEdit(
                                            family, entry.childId.copyOf(), entry.id.copyOf(),
                                            activeShared, entry.startUtcMs, entry.offsetMinutes,
                                            intervalDurationMs = entry.endUtcMs!! - entry.startUtcMs,
                                        )
                                    }) { Text(stringResource(R.string.move_completed_session)) }
                                }
                                if (entry.kind == "feed.bottle" && entry.bottleMl != null) {
                                    OutlinedButton(onClick = {
                                        pendingBottleEdit = PendingBottleEdit(
                                            family,
                                            entry.childId.copyOf(),
                                            entry.id.copyOf(),
                                            activeShared,
                                            localizedEntered(context, entry.bottleEntered ?: entry.bottleMl.toString()),
                                            entry.bottleUnit ?: 1u.toUByte(),
                                            entry.bottleContent ?: 4u.toUByte(),
                                        )
                                    }) { Text(stringResource(R.string.edit_bottle)) }
                                }
                                if (entry.kind == "feed.breast" && entry.breastSegments?.all {
                                    (it.endUtcMs - it.startUtcMs) % 60_000L == 0L
                                } == true) {
                                    OutlinedButton(onClick = {
                                        val savedSegments = entry.breastSegments!!
                                        pendingBreastEdit = PendingBreastEdit(
                                            family, entry.childId.copyOf(), entry.id.copyOf(), activeShared,
                                            entry.startUtcMs, entry.offsetMinutes,
                                            savedSegments.map {
                                                it.side to ((it.endUtcMs - it.startUtcMs) / 60_000L).toString()
                                            },
                                            savedSegments.mapIndexed { index, segment ->
                                                if (index == 0) 0L else segment.startUtcMs - savedSegments[index - 1].endUtcMs
                                            },
                                        )
                                    }) { Text(stringResource(R.string.edit_breast)) }
                                }
                                if (entry.kind == "diaper" && entry.diaperKind != null) {
                                    OutlinedButton(onClick = {
                                        pendingDiaperEdit = PendingDiaperEdit(
                                            family,
                                            entry.childId.copyOf(),
                                            entry.id.copyOf(),
                                            activeShared,
                                            entry.diaperKind!!,
                                        )
                                    }) { Text(stringResource(R.string.edit_diaper)) }
                                }
                                if (entry.kind == "feed.solids" && entry.solidsFoods != null) {
                                    OutlinedButton(onClick = {
                                        pendingSolidsEdit = PendingSolidsEdit(
                                            family,
                                            entry.childId.copyOf(),
                                            entry.id.copyOf(),
                                            activeShared,
                                            entry.solidsFoods!!.joinToString("\n"),
                                            entry.solidsAmount.orEmpty(),
                                        )
                                    }) { Text(stringResource(R.string.edit_solids)) }
                                }
                                if (entry.kind == "pump") {
                                    OutlinedButton(onClick = {
                                        pendingPumpEdit = PendingPumpEdit(
                                            family, entry.childId.copyOf(), entry.id.copyOf(), activeShared,
                                            entry.pumpLeftMl?.toString().orEmpty(),
                                            entry.pumpRightMl?.toString().orEmpty(),
                                            entry.pumpTotalMl?.toString().orEmpty(),
                                        )
                                    }) { Text(stringResource(R.string.edit_pump)) }
                                }
                                if (entry.kind == "medication" && entry.medicationName != null) {
                                    OutlinedButton(onClick = {
                                        pendingMedicationEdit = PendingMedicationEdit(
                                            family, entry.childId.copyOf(), entry.id.copyOf(), activeShared,
                                            entry.medicationName.orEmpty(),
                                            entry.medicationDoseAmount.orEmpty(),
                                            entry.medicationDoseUnit.orEmpty(),
                                        )
                                    }) { Text(stringResource(R.string.edit_medication)) }
                                }
                                if (entry.kind == "growth") {
                                    OutlinedButton(onClick = {
                                        pendingGrowthEdit = PendingGrowthEdit(
                                            family, entry.childId.copyOf(), entry.id.copyOf(), activeShared,
                                            localizedEntered(context, entry.growthWeightEntered ?: entry.growthWeightG?.toString().orEmpty()),
                                            entry.growthWeightUnit ?: 10u.toUByte(),
                                            localizedEntered(context, entry.growthLengthEntered ?: entry.growthLengthMm?.toString().orEmpty()),
                                            entry.growthLengthUnit ?: 20u.toUByte(),
                                            localizedEntered(context, entry.growthHeadEntered ?: entry.growthHeadMm?.toString().orEmpty()),
                                            entry.growthHeadUnit ?: 20u.toUByte(),
                                        )
                                    }) { Text(stringResource(R.string.edit_growth)) }
                                }
                                if (entry.kind == "temperature" && entry.temperatureC != null) {
                                    OutlinedButton(onClick = {
                                        pendingTemperatureEdit = PendingTemperatureEdit(
                                            family, entry.childId.copyOf(), entry.id.copyOf(), activeShared,
                                            localizedEntered(context, entry.temperatureEntered ?: entry.temperatureC!!),
                                            entry.temperatureUnit ?: 30u.toUByte(),
                                        )
                                    }) { Text(stringResource(R.string.edit_temperature)) }
                                }
                                OutlinedButton(onClick = {
                                    pendingDelete = PendingActivityDelete(
                                        family,
                                        entry.childId.copyOf(),
                                        entry.id.copyOf(),
                                        activeShared,
                                    )
                                }) { Text(stringResource(R.string.delete_entry)) }
                                }
                            }
                        }
                    }
                }

                if (route == TrackerDestination.FAMILY) {
                Spacer(Modifier.height(8.dp))
                OutlinedButton(onClick = { showDataControls = !showDataControls }) {
                    Text(stringResource(if (showDataControls) R.string.hide_data_controls
                        else R.string.show_data_controls))
                }
                if (showDataControls) {
                Text(stringResource(R.string.backup_title), style = MaterialTheme.typography.titleLarge)
                if (!activeShared) completed?.let { saved ->
                    Text(stringResource(R.string.last_saved_at, savedTime(saved.atMs)))
                    if (revision > saved.revision) Text(stringResource(R.string.changes_after_save))
                }
                Text(stringResource(if (activeShared) R.string.shared_backup_description else if (protectBackup) R.string.protected_backup_description else R.string.backup_description))
                FilterChip(
                    selected = protectBackup,
                    onClick = { protectBackup = !protectBackup },
                    label = { Text(stringResource(R.string.protect_backup)) },
                )
                if (protectBackup) OutlinedTextField(
                    value = backupPassword,
                    onValueChange = { backupPassword = it },
                    label = { Text(stringResource(R.string.backup_password)) },
                    visualTransformation = PasswordVisualTransformation(),
                    singleLine = true,
                )
                Button(onClick = {
                    scope.launch {
                        val password = backupPassword
                        val protected = protectBackup
                        runCatching { withContext(Dispatchers.IO) {
                            if (activeShared) sharing.backupFile(
                                family, System.currentTimeMillis(),
                                if (protected) password else null,
                                availableMemory().toULong(),
                            ) else store.backupFile(
                                family, System.currentTimeMillis(),
                                if (protected) password else null,
                                availableMemory().toULong(),
                            )
                        } }
                            .onSuccess {
                                pendingBackup = it
                                backupPassword = ""
                                saveLauncher.launch(if (protected) protectedFilename else filename)
                            }
                            .onFailure { message = errorText }
                    }
                }, enabled = !protectBackup || backupPassword.isNotEmpty()) { Text(stringResource(R.string.save_backup)) }
                OutlinedButton(onClick = {
                    scope.launch {
                        runCatching { withContext(Dispatchers.IO) {
                            if (activeShared) sharing.analysisCsv(family) else store.analysisCsv(family)
                        } }
                            .onSuccess { pendingAnalysisCsv = it; csvLauncher.launch("babytrack-analysis.csv") }
                            .onFailure { message = errorText }
                    }
                }) { Text(stringResource(R.string.export_analysis_csv)) }
                }
                }
            }
            if (route == TrackerDestination.FAMILY && (family == null || showDataControls)) {
            OutlinedButton(onClick = { restoreLauncher.launch(arrayOf("application/octet-stream", "*/*")) }) {
                Text(stringResource(R.string.restore_backup))
            }
            if (pendingRestore != null && pendingRestoreProtected) {
                OutlinedTextField(
                    value = restorePassword,
                    onValueChange = { restorePassword = it; restoreInfo = null },
                    label = { Text(stringResource(R.string.restore_password)) },
                    visualTransformation = PasswordVisualTransformation(),
                    singleLine = true,
                )
                Button(enabled = restorePassword.isNotEmpty(), onClick = {
                    val bytes = pendingRestore ?: return@Button
                    val password = restorePassword
                    scope.launch {
                        runCatching { withContext(Dispatchers.IO) {
                            store.inspectProtected(bytes, password, availableMemory().toULong())
                        } }.onSuccess { info ->
                            restoreInfo = info
                            message = null
                        }.onFailure { message = passwordOrFileError }
                    }
                }) { Text(stringResource(R.string.inspect_protected)) }
            }
            restoreInfo?.let { info ->
                Text(stringResource(R.string.file_saved_at, savedTime(info.snapshotUtcMs), info.recordCount.toLong()))
                if (info.knownGap) Text(stringResource(R.string.file_known_gap))
                Button(onClick = {
                    val bytes = pendingRestore ?: return@Button
                    val protected = pendingRestoreProtected
                    val password = restorePassword
                    scope.launch {
                        runCatching { withContext(Dispatchers.IO) {
                            if (protected) store.restoreProtected(
                                bytes, password, availableMemory().toULong(), System.currentTimeMillis()
                            ) else store.restore(bytes, System.currentTimeMillis())
                        } }.onSuccess { restored ->
                            pendingRestore = null
                            restoreInfo = null
                            restorePassword = ""
                            selectedFamily = restored.familyId.key()
                            selectedChild = null
                            version++
                            message = restoredText
                        }.onFailure { message = if (protected) passwordOrFileError else errorText }
                    }
                }) { Text(stringResource(R.string.confirm_restore)) }
            }
            }
            message?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        }
    }
    if (showAddChildForm && family != null) ChildProfileScreen(
        editing = false,
        name = childName,
        birthDate = childBirthDate,
        sex = childSex,
        saving = childProfileSaving,
        canClearBirthDate = true,
        onNameChange = { childName = it },
        onBirthDateChange = { childBirthDate = it },
        onSexChange = { childSex = it },
        onDismiss = { if (!childProfileSaving) showAddChildForm = false },
        onSave = save@{
            val name = childName.trim()
            if (name.isEmpty()) return@save
            val birthDay = runCatching {
                childBirthDate.takeIf { it.isNotBlank() }?.let { LocalDate.parse(it).toEpochDay() }
            }.getOrElse { message = context.getString(R.string.birth_date_invalid); return@save }
            childProfileSaving = true
            scope.launch {
                runCatching { withContext(Dispatchers.IO) {
                    if (activeShared) sharing.addChildWithMetadata(family, name, birthDay, childSex, System.currentTimeMillis())
                    else store.addChildWithMetadata(family, name, birthDay, childSex, System.currentTimeMillis())
                } }.onSuccess { created ->
                    selectedChild = created.key()
                    showAddChildForm = false
                    showChildDetails = false
                    childName = ""
                    childBirthDate = ""
                    childSex = 3u.toUByte()
                    version++
                    message = null
                }.onFailure { message = errorText }
                childProfileSaving = false
            }
        },
    )
    if (showTargetPicker) {
        AlertDialog(
            onDismissRequest = { showTargetPicker = false },
            title = { Text(stringResource(R.string.switch_target)) },
            text = {
                Column(Modifier.verticalScroll(rememberScrollState()),
                    verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(stringResource(R.string.families), style = MaterialTheme.typography.titleMedium)
                    families.forEachIndexed { index, item ->
                        val name = familyChildNames[item.familyId.key()]
                        val label = if (name == null) stringResource(R.string.family_number, index + 1)
                            else stringResource(R.string.family_with_child, index + 1, name)
                        FilterChip(selected = item.familyId.key() == selectedFamily,
                            onClick = {
                                selectedFamily = item.familyId.key()
                                selectedChild = null
                                showTargetPicker = false
                            }, label = { Text(label) })
                    }
                    if (children.isNotEmpty()) {
                        Text(stringResource(R.string.children), style = MaterialTheme.typography.titleMedium)
                        children.forEach { item ->
                            FilterChip(selected = item.id.key() == selectedChild,
                                onClick = {
                                    selectedChild = item.id.key()
                                    showTargetPicker = false
                                }, label = { Text(item.name) })
                        }
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = { showTargetPicker = false }) {
                    Text(stringResource(R.string.cancel))
                }
            },
        )
    }
    pendingRemovalNotice?.let { notice ->
        AlertDialog(
            onDismissRequest = {},
            title = { Text(stringResource(R.string.history_removed_copy_title)) },
            text = { Text(stringResource(
                R.string.history_removed_copy_destination,
                notice.sourceKey.take(8), notice.copy.familyId.key().take(8),
            )) },
            confirmButton = {
                Button(onClick = {
                    val copyKey = notice.copy.familyId.key()
                    if (selectionPrefs.edit().putString("family", copyKey)
                            .remove("child").commit() &&
                        removalNoticePrefs.edit().putString(notice.sourceKey, copyKey).commit()
                    ) {
                        selectedFamily = copyKey
                        selectedChild = null
                        pendingRemovalNotice = null
                        version++
                    } else message = errorText
                }) { Text(stringResource(R.string.continue_in_private_copy)) }
            },
        )
    }
    pendingDelete?.let { target ->
        AlertDialog(
            onDismissRequest = { pendingDelete = null },
            title = { Text(stringResource(R.string.delete_entry)) },
            text = { Text(stringResource(R.string.delete_entry_warning)) },
            confirmButton = {
                Button(onClick = {
                    pendingDelete = null
                    val savedAtMs = System.currentTimeMillis()
                    change(onSaved = { recentlyDeleted = target }) {
                        if (target.shared) sharing.deleteActivity(
                            target.family, target.childId, target.activityId, savedAtMs,
                        ) else store.deleteActivity(
                            target.family, target.childId, target.activityId, savedAtMs,
                        )
                    }
                }) { Text(stringResource(R.string.confirm_delete_entry)) }
            },
            dismissButton = {
                OutlinedButton(onClick = { pendingDelete = null }) {
                    Text(stringResource(R.string.cancel))
                }
            },
        )
    }
    pendingTimeEdit?.let { target ->
        AlertDialog(
            onDismissRequest = { pendingTimeEdit = null },
            title = { Text(stringResource(if (target.intervalDurationMs == null)
                R.string.edit_entry_time else R.string.move_completed_session)) },
            text = {
                Column {
                    Text(DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT)
                        .format(Date(target.startUtcMs)))
                    target.intervalDurationMs?.let { duration ->
                        val end = target.movedEndUtcMs ?: target.startUtcMs + duration
                        Text(stringResource(R.string.session_end_time,
                            DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(end))))
                    }
                    OutlinedButton(onClick = {
                        val current = java.util.Calendar.getInstance().apply { timeInMillis = target.startUtcMs }
                        DatePickerDialog(context, { _, year, month, day ->
                            TimePickerDialog(context, { _, hour, minute ->
                                if (pendingTimeEdit === target) {
                                    val selected = LocalDateTime.of(year, month + 1, day, hour, minute)
                                        .atZone(ZoneId.systemDefault()).toInstant().toEpochMilli()
                                    val movedEnd = target.intervalDurationMs?.let { duration ->
                                        runCatching { Math.addExact(selected, duration) }.getOrNull()
                                    }
                                    if (selected > System.currentTimeMillis() ||
                                        target.intervalDurationMs != null &&
                                        (movedEnd == null || movedEnd > System.currentTimeMillis())) {
                                        message = context.getString(R.string.log_time_future)
                                    } else {
                                        pendingTimeEdit = target.copy(
                                            startUtcMs = selected,
                                            offsetMinutes = (TimeZone.getDefault().getOffset(selected) / 60_000).toShort(),
                                            movedEndUtcMs = movedEnd,
                                            movedEndOffsetMinutes = movedEnd?.let {
                                                (TimeZone.getDefault().getOffset(it) / 60_000).toShort()
                                            },
                                        )
                                        message = null
                                    }
                                }
                            }, current.get(java.util.Calendar.HOUR_OF_DAY),
                                current.get(java.util.Calendar.MINUTE), false).show()
                        }, current.get(java.util.Calendar.YEAR), current.get(java.util.Calendar.MONTH),
                            current.get(java.util.Calendar.DAY_OF_MONTH)).apply {
                            datePicker.maxDate = System.currentTimeMillis()
                        }.show()
                    }) { Text(stringResource(R.string.choose_entry_time)) }
                }
            },
            confirmButton = {
                Button(enabled = target.intervalDurationMs == null ||
                    target.movedEndUtcMs != null && target.movedEndOffsetMinutes != null, onClick = {
                    val savedAtMs = System.currentTimeMillis()
                    change(onSaved = { pendingTimeEdit = null }) {
                        val end = target.movedEndUtcMs
                        val endOffset = target.movedEndOffsetMinutes
                        if (target.intervalDurationMs != null && end != null && endOffset != null) {
                            val time = ActivityWhen(target.startUtcMs, target.offsetMinutes, savedAtMs)
                            if (target.shared) sharing.moveCompletedInterval(
                                target.family, target.childId, target.activityId, time, end, endOffset,
                            ) else store.moveCompletedInterval(
                                target.family, target.childId, target.activityId, time, end, endOffset,
                            )
                        } else if (target.shared) sharing.editInstantTime(
                            target.family, target.childId, target.activityId,
                            target.startUtcMs, target.offsetMinutes, savedAtMs,
                        ) else store.editInstantTime(
                            target.family, target.childId, target.activityId,
                            ActivityWhen(target.startUtcMs, target.offsetMinutes, savedAtMs),
                        )
                    }
                }) { Text(stringResource(R.string.save_changes)) }
            },
            dismissButton = {
                OutlinedButton(onClick = { pendingTimeEdit = null }) {
                    Text(stringResource(R.string.cancel))
                }
            },
        )
    }
    pendingNoteEdit?.let { target ->
        AlertDialog(
            onDismissRequest = { pendingNoteEdit = null },
            title = { Text(stringResource(if (target.hadNote)
                R.string.edit_note else R.string.add_activity_note)) },
            text = {
                Column {
                    OutlinedTextField(
                        value = target.text,
                        onValueChange = { pendingNoteEdit = target.copy(text = it.take(4096)) },
                        label = { Text(stringResource(R.string.note_text)) },
                    )
                    if (!target.standalone && target.hadNote) {
                        Text(stringResource(R.string.activity_note_clear_hint))
                    }
                }
            },
            confirmButton = {
                Button(enabled = target.text.trim().isNotEmpty() ||
                    (!target.standalone && target.hadNote), onClick = {
                    val savedAtMs = System.currentTimeMillis()
                    change(onSaved = { pendingNoteEdit = null }) {
                        if (target.shared) sharing.editNote(
                            target.family, target.childId, target.activityId, target.text, savedAtMs,
                        ) else store.editNote(
                            target.family, target.childId, target.activityId, target.text, savedAtMs,
                        )
                    }
                }) { Text(stringResource(R.string.save_changes)) }
            },
            dismissButton = {
                OutlinedButton(onClick = { pendingNoteEdit = null }) {
                    Text(stringResource(R.string.cancel))
                }
            },
        )
    }
    pendingBottleEdit?.let { target ->
        AlertDialog(
            onDismissRequest = { pendingBottleEdit = null },
            title = { Text(stringResource(R.string.edit_bottle)) },
            text = {
                Column {
                    OutlinedTextField(
                        value = target.amount,
                        onValueChange = { pendingBottleEdit = target.copy(amount =
                            decimalDraft(it, target.unit != 1u.toUByte())) },
                        label = { Text(stringResource(R.string.bottle_amount)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        listOf(
                            1u.toUByte() to R.string.unit_ml,
                            2u.toUByte() to R.string.unit_us_fl_oz,
                            3u.toUByte() to R.string.unit_uk_fl_oz,
                        ).forEach { (unit, label) ->
                            FilterChip(selected = target.unit == unit,
                                onClick = {
                                    if (target.unit != unit) pendingBottleEdit = target.copy(amount = "", unit = unit)
                                },
                                label = { Text(stringResource(label)) })
                        }
                    }
                    listOf(
                        1u.toUByte() to R.string.bottle_breast_milk,
                        2u.toUByte() to R.string.bottle_formula,
                        3u.toUByte() to R.string.bottle_mixed,
                        4u.toUByte() to R.string.bottle_other,
                    ).chunked(2).forEach { options ->
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            options.forEach { (content, label) ->
                                FilterChip(selected = target.content == content,
                                    onClick = { pendingBottleEdit = target.copy(content = content) },
                                    label = { Text(stringResource(label)) })
                            }
                        }
                    }
                }
            },
            confirmButton = {
                Button(enabled = validBottleAmount(target.amount, target.unit), onClick = {
                    val savedAtMs = System.currentTimeMillis()
                    change(onSaved = { pendingBottleEdit = null }) {
                        if (target.shared) sharing.editBottleEntered(
                            target.family, target.childId, target.activityId,
                            canonicalDecimal(target.amount), target.unit, target.content, savedAtMs,
                        ) else store.editBottleEntered(
                            target.family, target.childId, target.activityId,
                            canonicalDecimal(target.amount), target.unit, target.content, savedAtMs,
                        )
                    }
                }) { Text(stringResource(R.string.save_changes)) }
            },
            dismissButton = {
                OutlinedButton(onClick = { pendingBottleEdit = null }) {
                    Text(stringResource(R.string.cancel))
                }
            },
        )
    }
    pendingBreastEdit?.let { target ->
        val durations = target.segments.map { it.second.toLongOrNull() }
        val valid = durations.all { it != null && it in 1L..240L } &&
            durations.filterNotNull().sum() <= 240L && target.segments.isNotEmpty()
        AlertDialog(
            onDismissRequest = { pendingBreastEdit = null },
            title = { Text(stringResource(R.string.edit_breast)) },
            text = {
                Column(Modifier.verticalScroll(rememberScrollState())) {
                    target.segments.forEachIndexed { index, segment ->
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            listOf(1u.toUByte() to R.string.breast_left,
                                2u.toUByte() to R.string.breast_right).forEach { (side, label) ->
                                FilterChip(selected = segment.first == side,
                                    onClick = {
                                        pendingBreastEdit = target.copy(segments = target.segments.mapIndexed { i, value ->
                                            if (i == index) side to value.second else value
                                        })
                                    }, label = { Text(stringResource(label)) })
                            }
                        }
                        OutlinedTextField(
                            value = segment.second,
                            onValueChange = { minutes ->
                                pendingBreastEdit = target.copy(segments = target.segments.mapIndexed { i, value ->
                                    if (i == index) value.first to minutes.filter(Char::isDigit).take(3) else value
                                })
                            },
                            label = { Text(stringResource(R.string.breast_minutes)) },
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                            singleLine = true,
                        )
                    }
                    if (target.segments.size > 1) {
                        OutlinedButton(onClick = { pendingBreastEdit = target.copy(
                            segments = target.segments.dropLast(1), gapsMs = target.gapsMs.dropLast(1),
                        ) }) {
                            Text(stringResource(R.string.remove_last_segment))
                        }
                    }
                    if (target.segments.size < 8) {
                        OutlinedButton(onClick = {
                            val nextSide = if (target.segments.last().first == 1u.toUByte()) 2u.toUByte() else 1u.toUByte()
                            pendingBreastEdit = target.copy(
                                segments = target.segments + (nextSide to "5"), gapsMs = target.gapsMs + 0L,
                            )
                        }) { Text(stringResource(R.string.add_breast_segment)) }
                    }
                }
            },
            confirmButton = {
                Button(enabled = valid, onClick = {
                    val savedAtMs = System.currentTimeMillis()
                    var cursor = target.startUtcMs
                    val zone = TimeZone.getDefault()
                    val segments = target.segments.mapIndexed { index, (side, minutes) ->
                        cursor += target.gapsMs[index]
                        val next = cursor + (durations[index] ?: error("Missing duration")) * 60_000L
                        BreastSegmentRow(side, cursor, next,
                            if (index == 0) target.startOffsetMinutes else (zone.getOffset(cursor) / 60_000).toShort(),
                            (zone.getOffset(next) / 60_000).toShort()).also { cursor = next }
                    }
                    pendingBreastEdit = null
                    change {
                        if (target.shared) sharing.editBreastFeedSegments(
                            target.family, target.childId, target.activityId, segments, savedAtMs,
                        ) else store.editBreastFeedSegments(
                            target.family, target.childId, target.activityId, segments, savedAtMs,
                        )
                    }
                }) { Text(stringResource(R.string.save_changes)) }
            },
            dismissButton = {
                OutlinedButton(onClick = { pendingBreastEdit = null }) { Text(stringResource(R.string.cancel)) }
            },
        )
    }
    pendingDiaperEdit?.let { target ->
        AlertDialog(
            onDismissRequest = { pendingDiaperEdit = null },
            title = { Text(stringResource(R.string.edit_diaper)) },
            text = {
                Column {
                    listOf(
                        1u.toUByte() to R.string.wet,
                        2u.toUByte() to R.string.dirty,
                        3u.toUByte() to R.string.both,
                        4u.toUByte() to R.string.dry,
                    ).forEach { (kind, label) ->
                        FilterChip(
                            selected = target.kind == kind,
                            onClick = { pendingDiaperEdit = target.copy(kind = kind) },
                            label = { Text(stringResource(label)) },
                        )
                    }
                }
            },
            confirmButton = {
                Button(onClick = {
                    pendingDiaperEdit = null
                    val savedAtMs = System.currentTimeMillis()
                    change {
                        if (target.shared) sharing.editDiaperKind(
                            target.family, target.childId, target.activityId, target.kind, savedAtMs,
                        ) else store.editDiaperKind(
                            target.family, target.childId, target.activityId, target.kind, savedAtMs,
                        )
                    }
                }) { Text(stringResource(R.string.save_changes)) }
            },
            dismissButton = {
                OutlinedButton(onClick = { pendingDiaperEdit = null }) {
                    Text(stringResource(R.string.cancel))
                }
            },
        )
    }
    pendingSolidsEdit?.let { target ->
        AlertDialog(
            onDismissRequest = { pendingSolidsEdit = null },
            title = { Text(stringResource(R.string.edit_solids)) },
            text = {
                Column {
                    OutlinedTextField(
                        value = target.foods,
                        onValueChange = { pendingSolidsEdit = target.copy(foods = it.take(2048)) },
                        label = { Text(stringResource(R.string.solids_foods)) },
                    )
                    OutlinedTextField(
                        value = target.amount,
                        onValueChange = { pendingSolidsEdit = target.copy(amount = it.take(256)) },
                        label = { Text(stringResource(R.string.solids_amount)) },
                    )
                }
            },
            confirmButton = {
                Button(enabled = target.foods.isNotBlank(), onClick = {
                    pendingSolidsEdit = null
                    val savedAtMs = System.currentTimeMillis()
                    val foods = target.foods.lines().map { it.trim() }.filter { it.isNotEmpty() }
                    change {
                        if (target.shared) sharing.editSolids(
                            target.family, target.childId, target.activityId, foods, target.amount, savedAtMs,
                        ) else store.editSolids(
                            target.family, target.childId, target.activityId, foods, target.amount, savedAtMs,
                        )
                    }
                }) { Text(stringResource(R.string.save_changes)) }
            },
            dismissButton = {
                OutlinedButton(onClick = { pendingSolidsEdit = null }) {
                    Text(stringResource(R.string.cancel))
                }
            },
        )
    }
    pendingGrowthEdit?.let { target ->
        AlertDialog(
            onDismissRequest = { pendingGrowthEdit = null },
            title = { Text(stringResource(R.string.edit_growth)) },
            text = {
                Column(modifier = Modifier.verticalScroll(rememberScrollState())) {
                    GrowthUnitChoices(R.string.weight_unit, massUnits, target.weightUnit) {
                        if (target.weightUnit != it) pendingGrowthEdit = target.copy(weight = "", weightUnit = it)
                    }
                    OutlinedTextField(
                        value = target.weight,
                        onValueChange = { pendingGrowthEdit = target.copy(weight = decimalDraft(it, target.weightUnit != 10u.toUByte())) },
                        label = { Text(stringResource(R.string.weight)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                        singleLine = true,
                    )
                    GrowthUnitChoices(R.string.length_unit, lengthUnits, target.lengthUnit) {
                        if (target.lengthUnit != it) pendingGrowthEdit = target.copy(length = "", lengthUnit = it)
                    }
                    OutlinedTextField(
                        value = target.length,
                        onValueChange = { pendingGrowthEdit = target.copy(length = decimalDraft(it, target.lengthUnit != 20u.toUByte())) },
                        label = { Text(stringResource(R.string.length)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                        singleLine = true,
                    )
                    GrowthUnitChoices(R.string.head_unit, lengthUnits, target.headUnit) {
                        if (target.headUnit != it) pendingGrowthEdit = target.copy(head = "", headUnit = it)
                    }
                    OutlinedTextField(
                        value = target.head,
                        onValueChange = { pendingGrowthEdit = target.copy(head = decimalDraft(it, target.headUnit != 20u.toUByte())) },
                        label = { Text(stringResource(R.string.head_circumference)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                        singleLine = true,
                    )
                    Text(stringResource(R.string.growth_edit_hint))
                }
            },
            confirmButton = {
                Button(enabled = listOf(target.weight, target.length, target.head).any { it.isNotBlank() } &&
                    validGrowthAmount(target.weight, target.weightUnit) &&
                    validGrowthAmount(target.length, target.lengthUnit) &&
                    validGrowthAmount(target.head, target.headUnit), onClick = {
                    val input = growthInput(target.weight, target.weightUnit,
                        target.length, target.lengthUnit, target.head, target.headUnit)
                    val savedAtMs = System.currentTimeMillis()
                    change(onSaved = { pendingGrowthEdit = null }) {
                        if (target.shared) sharing.editGrowthEntered(
                            target.family, target.childId, target.activityId, input, savedAtMs,
                        ) else store.editGrowthEntered(
                            target.family, target.childId, target.activityId, input, savedAtMs,
                        )
                    }
                }) { Text(stringResource(R.string.save_changes)) }
            },
            dismissButton = {
                OutlinedButton(onClick = { pendingGrowthEdit = null }) { Text(stringResource(R.string.cancel)) }
            },
        )
    }
    pendingPumpEdit?.let { target ->
        AlertDialog(
            onDismissRequest = { pendingPumpEdit = null },
            title = { Text(stringResource(R.string.edit_pump)) },
            text = {
                Column {
                    OutlinedTextField(
                        value = target.left,
                        onValueChange = { pendingPumpEdit = target.copy(left = it.filter(Char::isDigit).take(6)) },
                        label = { Text(stringResource(R.string.pump_left_ml)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                        singleLine = true,
                    )
                    OutlinedTextField(
                        value = target.right,
                        onValueChange = { pendingPumpEdit = target.copy(right = it.filter(Char::isDigit).take(6)) },
                        label = { Text(stringResource(R.string.pump_right_ml)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                        singleLine = true,
                    )
                    OutlinedTextField(
                        value = target.total,
                        onValueChange = { pendingPumpEdit = target.copy(total = it.filter(Char::isDigit).take(6)) },
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
                val valid = if (total != null) target.left.isBlank() && target.right.isBlank() && total in 1L..1_000_000L
                    else (left ?: 0L) + (right ?: 0L) > 0L &&
                        (left == null || left in 0L..1_000_000L) &&
                        (right == null || right in 0L..1_000_000L)
                Button(enabled = valid, onClick = {
                    pendingPumpEdit = null
                    val input = PumpInput(left, right, total)
                    val savedAtMs = System.currentTimeMillis()
                    change {
                        if (target.shared) sharing.editPumpAmounts(
                            target.family, target.childId, target.activityId, input, savedAtMs,
                        ) else store.editPumpAmounts(
                            target.family, target.childId, target.activityId, input, savedAtMs,
                        )
                    }
                }) { Text(stringResource(R.string.save_changes)) }
            },
            dismissButton = {
                OutlinedButton(onClick = { pendingPumpEdit = null }) { Text(stringResource(R.string.cancel)) }
            },
        )
    }
    pendingMedicationEdit?.let { target ->
        AlertDialog(
            onDismissRequest = { pendingMedicationEdit = null },
            title = { Text(stringResource(R.string.edit_medication)) },
            text = {
                Column {
                    OutlinedTextField(
                        value = target.name,
                        onValueChange = { pendingMedicationEdit = target.copy(name = it.take(256)) },
                        label = { Text(stringResource(R.string.medication_name)) },
                        singleLine = true,
                    )
                    OutlinedTextField(
                        value = target.doseAmount,
                        onValueChange = { pendingMedicationEdit = target.copy(doseAmount = it.take(64)) },
                        label = { Text(stringResource(R.string.dose_amount)) },
                        singleLine = true,
                    )
                    OutlinedTextField(
                        value = target.doseUnit,
                        onValueChange = { pendingMedicationEdit = target.copy(doseUnit = it.take(64)) },
                        label = { Text(stringResource(R.string.dose_unit)) },
                        singleLine = true,
                    )
                }
            },
            confirmButton = {
                Button(enabled = target.name.isNotBlank() && target.doseAmount.isNotBlank() &&
                    target.doseUnit.isNotBlank(), onClick = {
                    pendingMedicationEdit = null
                    val input = MedicationInput(target.name.trim(), target.doseAmount.trim(), target.doseUnit.trim())
                    val savedAtMs = System.currentTimeMillis()
                    change {
                        if (target.shared) sharing.editMedication(
                            target.family, target.childId, target.activityId, input, savedAtMs,
                        ) else store.editMedication(
                            target.family, target.childId, target.activityId, input, savedAtMs,
                        )
                    }
                }) { Text(stringResource(R.string.save_changes)) }
            },
            dismissButton = {
                OutlinedButton(onClick = { pendingMedicationEdit = null }) { Text(stringResource(R.string.cancel)) }
            },
        )
    }
    pendingChildProfileEdit?.let { target ->
        ChildProfileScreen(
            editing = true,
            name = target.name,
            birthDate = target.birthDate,
            sex = target.sex,
            saving = childProfileSaving,
            canClearBirthDate = target.originalBirthDate.isBlank(),
            onNameChange = { pendingChildProfileEdit = target.copy(name = it) },
            onBirthDateChange = { pendingChildProfileEdit = target.copy(birthDate = it) },
            onSexChange = { pendingChildProfileEdit = target.copy(sex = it) },
            onDismiss = { if (!childProfileSaving) pendingChildProfileEdit = null },
            onSave = save@{
                val name = target.name.trim()
                if (name.isEmpty()) return@save
                val birthDay = runCatching {
                    target.birthDate.takeIf { it.isNotBlank() }?.let { LocalDate.parse(it).toEpochDay() }
                }.getOrElse { message = context.getString(R.string.birth_date_invalid); return@save }
                if (name == target.originalName && target.birthDate == target.originalBirthDate &&
                    target.sex == target.originalSex) {
                    pendingChildProfileEdit = null
                    return@save
                }
                childProfileSaving = true
                scope.launch {
                    runCatching { withContext(Dispatchers.IO) {
                        if (name != target.originalName) {
                            if (target.shared) sharing.renameChild(target.family, target.childId, name, System.currentTimeMillis())
                            else store.renameChild(target.family, target.childId, name, System.currentTimeMillis())
                        }
                        if (target.birthDate != target.originalBirthDate || target.sex != target.originalSex) {
                            if (target.shared) sharing.editChildMetadata(
                                target.family, target.childId, birthDay, target.sex, System.currentTimeMillis(),
                            ) else store.editChildMetadata(
                                target.family, target.childId, birthDay, target.sex, System.currentTimeMillis(),
                            )
                        }
                    } }.onSuccess {
                        pendingChildProfileEdit = null
                        version++
                        message = null
                    }.onFailure {
                        version++
                        message = errorText
                    }
                    childProfileSaving = false
                }
            },
        )
    }
    pendingSleepEdit?.let { target ->
        AlertDialog(
            onDismissRequest = { pendingSleepEdit = null },
            title = { Text(stringResource(R.string.edit_sleep)) },
            text = {
                OutlinedTextField(
                    value = target.minutes,
                    onValueChange = { pendingSleepEdit = target.copy(minutes = it.filter(Char::isDigit).take(4)) },
                    label = { Text(stringResource(R.string.sleep_minutes)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    singleLine = true,
                )
            },
            confirmButton = {
                val minutes = target.minutes.toLongOrNull()
                val end = minutes?.let { target.startUtcMs + it * 60_000L }
                Button(enabled = minutes != null && minutes in 1L..1440L &&
                    end != null && end <= System.currentTimeMillis(), onClick = {
                    pendingSleepEdit = null
                    val savedAtMs = System.currentTimeMillis()
                    val newEnd = end ?: error("Sleep end missing")
                    val offset = (TimeZone.getDefault().getOffset(newEnd) / 60_000).toShort()
                    change {
                        if (target.shared) sharing.editSleepEnd(
                            target.family, target.childId, target.activityId, newEnd, offset, savedAtMs,
                        ) else store.editSleepEnd(
                            target.family, target.childId, target.activityId, newEnd, offset, savedAtMs,
                        )
                    }
                }) { Text(stringResource(R.string.save_changes)) }
            },
            dismissButton = {
                OutlinedButton(onClick = { pendingSleepEdit = null }) { Text(stringResource(R.string.cancel)) }
            },
        )
    }
    pendingSleepPlaceEdit?.let { target ->
        AlertDialog(
            onDismissRequest = { pendingSleepPlaceEdit = null },
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
                    ).chunked(2).forEach { options ->
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            options.forEach { (place, label) ->
                                FilterChip(
                                    selected = target.place == place,
                                    onClick = { pendingSleepPlaceEdit = target.copy(place = place) },
                                    label = { Text(stringResource(label)) },
                                )
                            }
                        }
                    }
                }
            },
            confirmButton = {
                Button(onClick = {
                    pendingSleepPlaceEdit = null
                    val savedAtMs = System.currentTimeMillis()
                    change {
                        if (target.shared) sharing.editSleepPlace(
                            target.family, target.childId, target.activityId, target.place, savedAtMs,
                        ) else store.editSleepPlace(
                            target.family, target.childId, target.activityId, target.place, savedAtMs,
                        )
                    }
                }) { Text(stringResource(R.string.save_changes)) }
            },
            dismissButton = {
                OutlinedButton(onClick = { pendingSleepPlaceEdit = null }) {
                    Text(stringResource(R.string.cancel))
                }
            },
        )
    }
    pendingTemperatureEdit?.let { target ->
        AlertDialog(
            onDismissRequest = { pendingTemperatureEdit = null },
            title = { Text(stringResource(R.string.edit_temperature)) },
            text = {
                Column {
                    OutlinedTextField(
                        value = target.entered,
                        onValueChange = { pendingTemperatureEdit = target.copy(entered = decimalDraft(it, fractional = true, signed = true)) },
                        label = { Text(stringResource(if (target.unit == 30u.toUByte())
                            R.string.temperature_c else R.string.temperature_f)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                        singleLine = true,
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        listOf(30u.toUByte() to R.string.unit_celsius,
                            31u.toUByte() to R.string.unit_fahrenheit).forEach { (unit, label) ->
                            FilterChip(selected = target.unit == unit, onClick = {
                                if (target.unit != unit) pendingTemperatureEdit = target.copy(entered = "", unit = unit)
                            }, label = { Text(stringResource(label)) })
                        }
                    }
                }
            },
            confirmButton = {
                Button(enabled = validTemperature(target.entered), onClick = {
                    val savedAtMs = System.currentTimeMillis()
                    change(onSaved = { pendingTemperatureEdit = null }) {
                        if (target.shared) sharing.editTemperatureEntered(
                            target.family, target.childId, target.activityId,
                            canonicalDecimal(target.entered), target.unit, savedAtMs,
                        ) else store.editTemperatureEntered(
                            target.family, target.childId, target.activityId,
                            canonicalDecimal(target.entered), target.unit, savedAtMs,
                        )
                    }
                }) { Text(stringResource(R.string.save_changes)) }
            },
            dismissButton = {
                OutlinedButton(onClick = { pendingTemperatureEdit = null }) { Text(stringResource(R.string.cancel)) }
            },
        )
    }
    cancelInvitationTarget?.let { target ->
        AlertDialog(
            onDismissRequest = { cancelInvitationTarget = null },
            title = { Text(stringResource(R.string.cancel_invitation_title)) },
            text = { Text(stringResource(R.string.cancel_invitation_warning, target.invitationId.key())) },
            confirmButton = {
                Button(onClick = {
                    cancelInvitationTarget = null
                    scope.launch {
                        runCatching { withContext(Dispatchers.IO) {
                            val origin = if (target.localManager)
                                lastRelayOrigin(target.family) ?: error("Relay origin unavailable")
                            else sharing.recipientOrigin(target.family)
                            sharing.cancelInvitation(target.family, origin, target.invitationId)
                        } }.onSuccess {
                            invitationFragment = null
                            version++
                            message = context.getString(R.string.invitation_canceled)
                        }.onFailure { message = errorText }
                    }
                }) { Text(stringResource(R.string.confirm_cancel_invitation)) }
            },
            dismissButton = {
                OutlinedButton(onClick = { cancelInvitationTarget = null }) {
                    Text(stringResource(R.string.cancel))
                }
            },
        )
    }
    pendingDeviceRemoval?.let { target ->
        val label = deviceLabels[deviceLabelKey(target.family.familyId, target.deviceId)]
            ?.takeIf { it.isNotBlank() } ?: target.deviceId.key().take(8)
        AlertDialog(
            onDismissRequest = { pendingDeviceRemoval = null },
            title = { Text(stringResource(R.string.remove_pending_device_title)) },
            text = { Text(stringResource(R.string.remove_pending_device_warning, label)) },
            confirmButton = {
                Button(onClick = {
                    pendingDeviceRemoval = null
                    scope.launch {
                        runCatching { withContext(Dispatchers.IO) {
                            val origin = if (target.localManager)
                                lastRelayOrigin(target.family) ?: error("Relay origin unavailable")
                            else sharing.recipientOrigin(target.family)
                            sharing.removePendingDevice(
                                target.family, origin, target.invitationId, target.deviceId,
                            )
                        } }.onSuccess {
                            activeSharedSnapshot = it
                            version++
                            message = context.getString(R.string.pending_device_removed)
                        }.onFailure { message = errorText }
                    }
                }) { Text(stringResource(R.string.confirm_remove_pending_device)) }
            },
            dismissButton = {
                OutlinedButton(onClick = { pendingDeviceRemoval = null }) {
                    Text(stringResource(R.string.cancel))
                }
            },
        )
    }
    roleChangeTarget?.let { target ->
        val label = deviceLabels[deviceLabelKey(target.family.familyId, target.targetId)]
            ?.takeIf { it.isNotBlank() } ?: target.targetId.key().take(8)
        AlertDialog(
            onDismissRequest = { roleChangeTarget = null },
            title = { Text(stringResource(R.string.change_device_role_title)) },
            text = { Text(stringResource(R.string.change_device_role_warning, label,
                if (target.newRole == 2.toUByte()) stringResource(R.string.manager_role)
                else stringResource(R.string.member_role))) },
            confirmButton = {
                Button(onClick = {
                    roleChangeTarget = null
                    scope.launch {
                        runCatching { withContext(Dispatchers.IO) {
                            val origin = if (target.localManager)
                                lastRelayOrigin(target.family) ?: error("Relay origin unavailable")
                            else sharing.recipientOrigin(target.family)
                            sharing.changeDeviceRole(target.family, origin, target.targetId, target.newRole)
                        } }.onSuccess {
                            activeSharedSnapshot = it
                            version++
                            message = context.getString(R.string.device_role_changed)
                        }.onFailure { message = errorText }
                    }
                }) { Text(stringResource(R.string.confirm_device_role_change)) }
            },
            dismissButton = {
                OutlinedButton(onClick = { roleChangeTarget = null }) {
                    Text(stringResource(R.string.cancel))
                }
            },
        )
    }
    removalTarget?.let { target ->
        val targetLabel = family?.let { deviceLabels[deviceLabelKey(it.familyId, target)] }
            ?.takeIf { it.isNotBlank() }
        val targetDescription = if (targetLabel == null) target.key()
            else context.getString(R.string.named_device_id, targetLabel, target.key())
        AlertDialog(
            onDismissRequest = { removalTarget = null },
            title = { Text(stringResource(R.string.remove_device_title)) },
            text = { Text(stringResource(R.string.remove_device_warning, targetDescription)) },
            confirmButton = {
                Button(onClick = {
                    removalTarget = null
                    val chosen = family ?: return@Button
                    scope.launch {
                        runCatching { withContext(Dispatchers.IO) {
                            sharing.removeDevice(chosen, relayOrigin.trim(), target)
                        } }.onSuccess {
                            activeSharedSnapshot = it
                            version++
                            message = context.getString(R.string.device_removed)
                        }.onFailure { message = errorText }
                    }
                }) { Text(stringResource(R.string.confirm_remove_device)) }
            },
            dismissButton = {
                OutlinedButton(onClick = { removalTarget = null }) {
                    Text(stringResource(R.string.cancel))
                }
            },
        )
    }
    deviceLabelTarget?.let { key ->
        AlertDialog(
            onDismissRequest = { deviceLabelTarget = null },
            title = { Text(stringResource(R.string.device_label_title)) },
            text = {
                OutlinedTextField(
                    value = deviceLabelDraft,
                    onValueChange = { deviceLabelDraft = it.take(40) },
                    label = { Text(stringResource(R.string.device_label_hint)) },
                    singleLine = true,
                )
            },
            confirmButton = {
                Button(onClick = {
                    val label = deviceLabelDraft.trim()
                    if (label.isEmpty()) {
                        deviceLabelPrefs.edit().remove(key).apply()
                        deviceLabels = deviceLabels - key
                    } else {
                        deviceLabelPrefs.edit().putString(key, label).apply()
                        deviceLabels = deviceLabels + (key to label)
                    }
                    deviceLabelTarget = null
                }) { Text(stringResource(R.string.save_changes)) }
            },
            dismissButton = {
                OutlinedButton(onClick = { deviceLabelTarget = null }) {
                    Text(stringResource(R.string.cancel))
                }
            },
        )
    }
}

private fun nowTime(): ActivityWhen {
    return activityWhen(System.currentTimeMillis())
}

private fun activityWhen(atMs: Long): ActivityWhen = ActivityWhen(
    atMs,
    (TimeZone.getDefault().getOffset(atMs) / 60_000).toShort(),
    System.currentTimeMillis(),
)

private fun terminalInvitationMessage(context: Context, reason: InvitationTerminalReason): String =
    context.getString(when (reason) {
        InvitationTerminalReason.CLAIMED -> R.string.join_claimed
        InvitationTerminalReason.CANCELED -> R.string.join_canceled
        InvitationTerminalReason.EXPIRED -> R.string.join_expired
        InvitationTerminalReason.ISSUER_INVALID -> R.string.join_issuer_invalid
    })

@Composable
private fun SharedHealth(
    snapshot: SharedSnapshotRow,
    deviceLabels: Map<String, String>,
    onNameDevice: (ByteArray) -> Unit,
) {
    Text(stringResource(R.string.shared_devices), style = MaterialTheme.typography.titleMedium)
    snapshot.devices.forEach { device ->
        val role = stringResource(if (device.role == 2.toUByte()) R.string.invite_manager else R.string.invite_member)
        val who = if (device.deviceId.contentEquals(snapshot.family.deviceId)) {
            stringResource(R.string.this_device)
        } else {
            stringResource(R.string.another_device)
        }
        val label = deviceLabels[deviceLabelKey(snapshot.family.familyId, device.deviceId)]
            ?.takeIf { it.isNotBlank() }
        Text(if (label == null) stringResource(R.string.shared_device_row, who, role, device.deviceId.key())
            else stringResource(R.string.shared_named_device_row, label, who, role, device.deviceId.key()))
        OutlinedButton(onClick = { onNameDevice(device.deviceId) }) {
            Text(stringResource(R.string.name_device))
        }
    }
}

private fun sharedSyncMessage(context: Context, progress: SharedSyncRow): String = when {
    !progress.ready -> context.getString(R.string.history_pending, progress.verifiedCursor.toLong())
    progress.outboxState == 2.toUByte() -> context.getString(R.string.shared_upload_uncertain)
    progress.outboxState == 1.toUByte() -> context.getString(R.string.shared_upload_pending)
    else -> context.getString(R.string.shared_synced, progress.verifiedCursor.toLong())
}

private fun pendingRecipientMessage(
    context: Context,
    progress: uniffi.babytrack_core_ffi.RecipientSyncRow,
): String = when (progress.joinPhase.toInt()) {
    2 -> context.getString(R.string.history_awaiting_challenge)
    3 -> context.getString(R.string.history_challenge_received)
    4 -> context.getString(R.string.history_proof_committed)
    else -> context.getString(R.string.history_awaiting_grant, progress.pendingControlCursor.toLong())
}

private fun removedHistoryMessage(
    context: Context,
    progress: uniffi.babytrack_core_ffi.RecipientSyncRow,
): String = when {
    progress.privateCopy == null -> context.getString(R.string.history_removed)
    progress.pendingResult == 0.toUByte() -> context.getString(R.string.history_removed_unsent)
    progress.pendingResult == 2.toUByte() -> context.getString(R.string.history_removed_accepted)
    progress.pendingResult == 3.toUByte() -> context.getString(R.string.history_removed_rejected)
    else -> context.getString(R.string.history_removed_copied)
}

private fun savedTime(utcMs: Long): String =
    DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(utcMs))
