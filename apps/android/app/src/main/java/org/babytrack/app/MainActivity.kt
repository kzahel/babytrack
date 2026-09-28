package org.babytrack.app

import android.Manifest
import android.os.Bundle
import android.os.Build
import android.app.ActivityManager
import android.app.DatePickerDialog
import android.app.TimePickerDialog
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
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
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.Button
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
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
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInParent
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.unit.dp
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
import uniffi.babytrack_core_ffi.FamilyRef
import uniffi.babytrack_core_ffi.NativeLocalStore
import uniffi.babytrack_core_ffi.MedicationInput
import uniffi.babytrack_core_ffi.PumpInput
import uniffi.babytrack_core_ffi.RemovedDeviceRow
import uniffi.babytrack_core_ffi.RestoredOriginRow
import uniffi.babytrack_core_ffi.SharedSnapshotRow
import uniffi.babytrack_core_ffi.SharedSyncRow
import java.text.DateFormat
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.Instant
import java.time.ZoneId
import java.util.Date
import java.util.TimeZone
import kotlin.math.roundToInt

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
            MaterialTheme(colorScheme = if (isSystemInDarkTheme()) darkColorScheme() else lightColorScheme()) {
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

private fun ByteArray.key(): String = joinToString("") { "%02x".format(it) }

private fun deviceLabelKey(familyId: ByteArray, deviceId: ByteArray): String =
    familyId.key() + ":" + deviceId.key()

private data class CompletedSave(val atMs: Long, val revision: ULong)
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

private fun validBottleAmount(value: String, unit: UByte): Boolean =
    value.length <= 16 && bottleAmountPattern.matches(value) &&
        (unit != 1u.toUByte() || !value.contains('.')) &&
        value.any { it in '1'..'9' } &&
        (unit != 1u.toUByte() || value.toLongOrNull()?.let { it in 1..1_000_000 } == true)

private data class PendingBreastEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val startUtcMs: Long,
    val startOffsetMinutes: Short,
    val segments: List<Pair<UByte, String>>,
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
    val length: String,
    val head: String,
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
private data class PendingChildMetadataEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val shared: Boolean,
    val birthDate: String,
    val sex: UByte,
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
)

private data class PendingInvitationCancel(
    val family: FamilyRef,
    val invitationId: ByteArray,
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
    return ScreenData(
        shown, (removedLocal + removedRecipients).distinctBy { it.familyId.key() },
        familyChildNames, family?.familyId?.key(), localFamily, kids, history,
        if (!shared) family?.let(store::revision) ?: 0uL else 0uL,
        if (!shared) family?.let(store::restoredOrigin) else null,
        shared, snapshot, recipients, readyJoined.mapTo(mutableSetOf()) { it.first.familyId.key() },
        joinedSnapshot, unusedInvitationIds,
        runningSleepCount(store, sharing, activeLocal, recipients),
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
    var revision by remember { mutableStateOf(0uL) }
    var restoredOrigin by remember { mutableStateOf<RestoredOriginRow?>(null) }
    var isShared by remember { mutableStateOf(false) }
    var activeSharedSnapshot by remember { mutableStateOf<SharedSnapshotRow?>(null) }
    var activeUnusedInvitationIds by remember { mutableStateOf<List<ByteArray>?>(emptyList()) }
    var loadedFamilyKey by remember { mutableStateOf<String?>(null) }
    var activeFamilyIsLocal by remember { mutableStateOf(false) }
    var saveStatusVersion by remember { mutableStateOf(0) }
    val selectionPrefs = remember { context.getSharedPreferences("tracker_selection", Context.MODE_PRIVATE) }
    var selectedFamily by remember { mutableStateOf(selectionPrefs.getString("family", null)) }
    var selectedChild by remember { mutableStateOf(selectionPrefs.getString("child", null)) }
    LaunchedEffect(loadedFamilyKey, selectedFamily, selectedChild, children) {
        if (selectedFamily != null && loadedFamilyKey == selectedFamily) {
            val child = selectedChild.takeIf { chosen -> children.any { it.id.key() == chosen } }
            selectionPrefs.edit().putString("family", selectedFamily).putString("child", child).apply()
        }
    }
    var childName by remember { mutableStateOf("") }
    var showAddChildForm by remember { mutableStateOf(false) }
    var childRename by remember { mutableStateOf<String?>(null) }
    var pendingChildMetadataEdit by remember { mutableStateOf<PendingChildMetadataEdit?>(null) }
    var childBirthDate by remember { mutableStateOf("") }
    var childSex by remember { mutableStateOf(3u.toUByte()) }
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
    var growthLength by remember { mutableStateOf("") }
    var growthHead by remember { mutableStateOf("") }
    var temperatureEntered by remember { mutableStateOf("") }
    var temperatureUnit by remember { mutableStateOf(30u.toUByte()) }
    var medicationName by remember { mutableStateOf("") }
    var doseAmount by remember { mutableStateOf("") }
    var doseUnit by remember { mutableStateOf("") }
    var logAtMs by remember { mutableStateOf<Long?>(null) }
    var timelineFilter by remember { mutableStateOf(TimelineFilter.ALL) }
    var message by remember { mutableStateOf<String?>(null) }
    val snackbarHostState = remember { SnackbarHostState() }
    LaunchedEffect(message) {
        message?.let { snackbarHostState.showSnackbar(it) }
    }
    var automaticSyncDelayed by remember { mutableStateOf(false) }
    var automaticSyncBlocked by remember { mutableStateOf(false) }
    var removalTarget by remember { mutableStateOf<ByteArray?>(null) }
    var cancelInvitationTarget by remember { mutableStateOf<PendingInvitationCancel?>(null) }
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
    var pendingNoteEdit by remember { mutableStateOf<PendingNoteEdit?>(null) }
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
    var invitationFragment by remember { mutableStateOf<String?>(null) }
    var receivedFragment by remember { mutableStateOf("") }
    var showJoinForm by remember { mutableStateOf(false) }
    var showShareForm by remember { mutableStateOf(false) }
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
        }
    }
    val errorText = stringResource(R.string.error)
    val savedText = stringResource(R.string.saved)
    val restoredText = stringResource(R.string.restored)
    LaunchedEffect(selectedFamily) {
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
        growthLength = ""
        growthHead = ""
        temperatureEntered = ""
        temperatureUnit = 30u.toUByte()
        medicationName = ""
        doseAmount = ""
        doseUnit = ""
        childRename = null
        pendingChildMetadataEdit = null
        pendingDelete = null
        pendingNoteEdit = null
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
                for (family in store.families()) {
                    val origin = lastRelayOrigin(family)
                    if (origin != null && sharing.isShared(family) && !sharing.isRemoved(family)) {
                        runCatching { sharing.advanceManager(family, origin) }
                            .onFailure {
                                if (it is VerifiedManagerRemoval) removals[family.familyId.key()] = it.result
                                else { failed = true; if (it is SharedUploadBlocked) blocked = true }
                            }
                    }
                }
                for (family in sharing.recipientFamilies()) {
                    runCatching { sharing.advanceRecipient(family) }
                        .onSuccess { stages[family.familyId.key()] = it }
                        .onFailure {
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
                removed.privateCopy?.let { copy ->
                    selectedFamily = copy.familyId.key()
                    selectedChild = null
                }
                message = when {
                    removed.privateCopy == null -> context.getString(R.string.history_removed)
                    removed.pendingResult == 2.toUByte() -> context.getString(R.string.history_removed_accepted)
                    removed.pendingResult == 3.toUByte() -> context.getString(R.string.history_removed_rejected)
                    removed.pendingResult == 0.toUByte() -> context.getString(R.string.history_removed_unsent)
                    else -> context.getString(R.string.history_removed_copied)
                }
            }
            recipientStages[selectedRecipient]?.let { progress ->
                if (progress.removed) sharedSnapshot = null
                joinStage = when {
                    progress.removed -> removedHistoryMessage(context, progress)
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
            revision = data.revision
            restoredOrigin = data.restoredOrigin
            isShared = data.shared
            activeSharedSnapshot = data.mainSharedSnapshot
            activeUnusedInvitationIds = data.unusedInvitationIds
            loadedFamilyKey = data.activeFamilyKey
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
    val activeShared = isShared && loadedFamilyKey == selectedFamily
    val completed = remember(selectedFamily, saveStatusVersion) { family?.let(lastSave) }
    val filename = stringResource(R.string.backup_filename)
    val protectedFilename = stringResource(R.string.protected_backup_filename)
    val scrollState = rememberScrollState()
    LaunchedEffect(incomingInvitation) {
        if (incomingInvitation != null) scrollState.scrollTo(0)
    }
    val joinFirst = incomingInvitation != null &&
        (receivedFragment.isNotBlank() || sharedSnapshot == null)
    var timelineTop by remember { mutableStateOf(0) }
    fun logTime(): ActivityWhen = activityWhen(logAtMs ?: System.currentTimeMillis())
    fun resetLogTime(savedAt: Long?) {
        if (logAtMs == savedAt) logAtMs = null
    }
    fun logCompleted(onSaved: (() -> Unit)? = null, action: (ActivityWhen) -> Unit) {
        val chosenAt = logAtMs
        val at = logTime()
        change(onSaved = { resetLogTime(chosenAt); onSaved?.invoke() }) { action(at) }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    val familyNumber = families.indexOfFirst { it.familyId.key() == selectedFamily } + 1
                    Text(
                        if (child != null && familyNumber > 0) {
                            stringResource(R.string.family_with_child, familyNumber, child.name)
                        } else stringResource(R.string.screen_title),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                },
                actions = {
                    if (family != null && child != null) {
                        val description = stringResource(R.string.quick_wet_diaper_description)
                        TextButton(
                            modifier = Modifier.semantics { contentDescription = description },
                            onClick = {
                                change {
                                    val at = nowTime()
                                    if (activeShared) sharing.logDiaper(family, child.id, 1u.toUByte(), at)
                                    else store.logDiaper(family, child.id, 1u.toUByte(), at)
                                }
                            },
                        ) { Text(stringResource(R.string.quick_wet_diaper)) }
                    }
                },
            )
        },
        snackbarHost = { SnackbarHost(snackbarHostState) },
    ) { padding ->
        val joinControls: @Composable () -> Unit = {
            if (BuildConfig.DEBUG) {
                if (!showJoinForm && recipientFamilies.isEmpty()) OutlinedButton(
                    onClick = { showJoinForm = true },
                ) { Text(stringResource(R.string.join_family)) }
                else Card(modifier = Modifier.fillMaxWidth()) {
                    Column(
                        modifier = Modifier.padding(16.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Text(stringResource(R.string.dev_join_title), style = MaterialTheme.typography.titleMedium)
                        Text(stringResource(R.string.dev_join_description))
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
        }
        Column(
            modifier = Modifier.fillMaxSize().padding(padding).verticalScroll(scrollState).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            if (joinFirst) joinControls()
            Text(stringResource(if (activeShared) R.string.shared_family else R.string.local_only), style = MaterialTheme.typography.labelMedium)
            if (automaticSyncDelayed && !automaticSyncBlocked) Text(stringResource(R.string.automatic_sync_delayed))
            if (automaticSyncBlocked) Text(
                stringResource(R.string.shared_upload_blocked),
                color = MaterialTheme.colorScheme.error,
            )
            if (activeShared) activeSharedSnapshot?.let { snapshot ->
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
                    if (BuildConfig.DEBUG && !activeFamilyIsLocal) {
                        Text(stringResource(R.string.invite_manager), style = MaterialTheme.typography.titleMedium)
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
                        OutlinedButton(onClick = {
                            shareStage = context.getString(R.string.invite_preparing)
                            scope.launch {
                                runCatching { withContext(Dispatchers.IO) {
                                    sharing.invite(
                                        snapshot.family,
                                        sharing.recipientOrigin(snapshot.family),
                                        if (inviteAsManager) 2u.toUByte() else 1u.toUByte(),
                                    )
                                } }.onSuccess { fragment ->
                                    invitationFragment = fragment
                                    shareStage = context.getString(R.string.invite_confirmed)
                                    version++
                                    message = null
                                }.onFailure {
                                    shareStage = context.getString(R.string.share_retry)
                                    message = errorText
                                }
                            }
                        }) { Text(stringResource(R.string.create_invite)) }
                        shareStage?.let { Text(it) }
                        invitationFragment?.let { fragment ->
                            SelectionContainer { Text(fragment) }
                            OutlinedButton(onClick = {
                                val send = Intent(Intent.ACTION_SEND).apply {
                                    type = "text/plain"
                                    putExtra(Intent.EXTRA_TEXT, invitationLink(fragment))
                                }
                                context.startActivity(Intent.createChooser(
                                    send, context.getString(R.string.share_invitation),
                                ))
                            }) { Text(stringResource(R.string.share_invitation)) }
                        }
                    }
                }
            }
            if (activeShared && family != null) OutlinedButton(onClick = {
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
            Text(stringResource(R.string.families), style = MaterialTheme.typography.titleLarge)
            families.forEachIndexed { index, item ->
                FilterChip(
                    selected = item.familyId.key() == selectedFamily,
                    onClick = { selectedFamily = item.familyId.key(); selectedChild = null },
                    label = {
                        val firstChild = familyChildNames[item.familyId.key()]
                        Text(if (firstChild == null) stringResource(R.string.family_number, index + 1)
                            else stringResource(R.string.family_with_child, index + 1, firstChild))
                    },
                )
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
            OutlinedButton(onClick = {
                scope.launch {
                    runCatching { withContext(Dispatchers.IO) { store.createFamily(System.currentTimeMillis()) } }
                        .onSuccess { created ->
                            selectedFamily = created.familyId.key()
                            selectedChild = null
                            version++
                            message = null
                        }.onFailure { message = errorText }
                }
            }) { Text(stringResource(R.string.new_family)) }

            if (!joinFirst) joinControls()

            if (family != null && activeFamilyIsLocal) {
                if (BuildConfig.DEBUG) {
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
                            OutlinedButton(onClick = {
                                shareStage = context.getString(R.string.invite_preparing)
                                scope.launch {
                                    runCatching {
                                        withContext(Dispatchers.IO) {
                                            sharing.invite(
                                                family,
                                                relayOrigin.trim(),
                                                if (inviteAsManager) 2u.toUByte() else 1u.toUByte(),
                                            )
                                        }
                                    }.onSuccess { fragment ->
                                        invitationFragment = fragment
                                        shareStage = context.getString(R.string.invite_confirmed)
                                        version++
                                        message = null
                                    }.onFailure {
                                        shareStage = context.getString(R.string.share_retry)
                                        message = errorText
                                    }
                                }
                            }) { Text(stringResource(R.string.create_invite)) }
                            invitationFragment?.let { fragment ->
                                Text(stringResource(R.string.invite_fragment_label))
                                SelectionContainer { Text(fragment) }
                                OutlinedButton(onClick = {
                                    val send = Intent(Intent.ACTION_SEND).apply {
                                        type = "text/plain"
                                        putExtra(Intent.EXTRA_TEXT, invitationLink(fragment))
                                    }
                                    context.startActivity(Intent.createChooser(
                                        send,
                                        context.getString(R.string.share_invitation),
                                    ))
                                }) { Text(stringResource(R.string.share_invitation)) }
                            }
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
                restoredOrigin?.let { origin ->
                    Text(stringResource(R.string.restored_from, savedTime(origin.snapshotUtcMs)))
                    if (origin.knownGap) Text(stringResource(R.string.file_known_gap))
                }
                Text(stringResource(R.string.children), style = MaterialTheme.typography.titleLarge)
                if (children.isEmpty()) Text(stringResource(R.string.no_children))
                children.forEach { item ->
                    FilterChip(
                        selected = item.id.key() == selectedChild,
                        onClick = { selectedChild = item.id.key() },
                        label = { Text(item.name) },
                    )
                }
                if (child != null) {
                    if (childRename == null) {
                        OutlinedButton(onClick = { childRename = child.name }) {
                            Text(stringResource(R.string.rename_child))
                        }
                    } else {
                        OutlinedTextField(
                            value = childRename.orEmpty(),
                            onValueChange = { childRename = it.take(16 * 1024) },
                            label = { Text(stringResource(R.string.new_child_name)) },
                            modifier = Modifier.fillMaxWidth(),
                            singleLine = true,
                        )
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            Button(enabled = !childRename.isNullOrBlank(), onClick = {
                                val name = childRename?.trim() ?: return@Button
                                change(onSaved = { childRename = null }) {
                                    if (activeShared) sharing.renameChild(family, child.id, name, System.currentTimeMillis())
                                    else store.renameChild(family, child.id, name, System.currentTimeMillis())
                                }
                            }) { Text(stringResource(R.string.save_changes)) }
                            OutlinedButton(onClick = { childRename = null }) {
                                Text(stringResource(R.string.cancel))
                            }
                        }
                    }
                    OutlinedButton(onClick = {
                        pendingChildMetadataEdit = PendingChildMetadataEdit(
                            family, child.id.copyOf(), activeShared,
                            child.birthDay?.let { day ->
                                runCatching { LocalDate.ofEpochDay(day).toString() }.getOrDefault("")
                            }.orEmpty(),
                            child.sex ?: 3u.toUByte(),
                        )
                    }) { Text(stringResource(R.string.edit_child_growth_details)) }
                }
                if (children.isNotEmpty() && !showAddChildForm) OutlinedButton(onClick = {
                    showAddChildForm = true
                }) { Text(stringResource(R.string.add_another_child)) }
                if (children.isEmpty() || showAddChildForm) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedTextField(
                            value = childName,
                            onValueChange = { childName = it },
                            label = { Text(stringResource(R.string.child_name)) },
                            modifier = Modifier.weight(1f),
                            singleLine = true,
                        )
                        Button(enabled = childName.isNotBlank(), onClick = {
                            val name = childName.trim()
                            val birthDay = runCatching { childBirthDate.takeIf { it.isNotBlank() }?.let { LocalDate.parse(it).toEpochDay() } }
                                .getOrElse { message = context.getString(R.string.birth_date_invalid); return@Button }
                            scope.launch {
                                runCatching { withContext(Dispatchers.IO) {
                                    if (activeShared) sharing.addChildWithMetadata(family, name, birthDay, childSex, System.currentTimeMillis())
                                    else store.addChildWithMetadata(family, name, birthDay, childSex, System.currentTimeMillis())
                                } }
                                    .onSuccess { created ->
                                        selectedChild = created.key()
                                        showAddChildForm = false
                                        childName = ""
                                        childBirthDate = ""
                                        childSex = 3u.toUByte()
                                        version++
                                        message = null
                                    }.onFailure { message = errorText }
                            }
                        }) { Text(stringResource(R.string.add_child)) }
                    }
                    OutlinedTextField(
                        value = childBirthDate,
                        onValueChange = { childBirthDate = it },
                        label = { Text(stringResource(R.string.birth_date)) },
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                    )
                    Text(stringResource(R.string.growth_chart_sex))
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        listOf(
                            1u.toUByte() to R.string.sex_female,
                            2u.toUByte() to R.string.sex_male,
                            3u.toUByte() to R.string.sex_unspecified,
                        ).forEach { (code, label) ->
                            FilterChip(
                                selected = childSex == code,
                                onClick = { childSex = code },
                                label = { Text(stringResource(label)) },
                            )
                        }
                    }
                    if (children.isNotEmpty()) OutlinedButton(onClick = {
                        showAddChildForm = false
                        childName = ""
                        childBirthDate = ""
                        childSex = 3u.toUByte()
                    }) { Text(stringResource(R.string.cancel)) }
                }

                if (child != null) {
                    OutlinedButton(onClick = {
                        scope.launch { scrollState.animateScrollTo(timelineTop) }
                    }) { Text(stringResource(R.string.view_timeline)) }
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
                            onValueChange = { amount = it.filter { char ->
                                char.isDigit() || (bottleUnit != 1u.toUByte() && char == '.')
                            }.take(16) },
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
                                if (activeShared) sharing.logBottleEntered(family, child.id, entered, unit, content, at)
                                else store.logBottleEntered(family, child.id, entered, unit, content, at)
                            }
                        }) { Text(stringResource(R.string.log_bottle)) }
                    }
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
                            }.onFailure { message = errorText }
                        }
                    }) { Text(stringResource(R.string.save_breast)) }
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
                            }.onFailure { message = errorText }
                        }
                    }) { Text(stringResource(R.string.save_pump)) }
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
                            }.onFailure { message = errorText }
                        }
                    }) { Text(stringResource(R.string.save_solids)) }
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
                                if (sleepMinutes == enteredMinutes) sleepMinutes = ""
                                if (sleepPlace == enteredPlace) sleepPlace = null
                            }) {
                                if (activeShared) sharing.logSleepWithPlace(family, child.id, whenStarted, end, endOffset, enteredPlace)
                                else store.logSleepWithPlace(family, child.id, whenStarted, end, endOffset, enteredPlace)
                            }
                        }) { Text(stringResource(R.string.save_sleep)) }
                    }
                    Text(stringResource(R.string.log_growth), style = MaterialTheme.typography.titleLarge)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedTextField(
                            value = growthWeight,
                            onValueChange = { growthWeight = it.filter(Char::isDigit) },
                            label = { Text(stringResource(R.string.weight_g)) },
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                            modifier = Modifier.weight(1f),
                            singleLine = true,
                        )
                        OutlinedTextField(
                            value = growthLength,
                            onValueChange = { growthLength = it.filter(Char::isDigit) },
                            label = { Text(stringResource(R.string.length_mm)) },
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                            modifier = Modifier.weight(1f),
                            singleLine = true,
                        )
                    }
                    OutlinedTextField(
                        value = growthHead,
                        onValueChange = { growthHead = it.filter(Char::isDigit).take(4) },
                        label = { Text(stringResource(R.string.head_mm)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                    )
                    val weight = growthWeight.toLongOrNull()
                    val length = growthLength.toLongOrNull()
                    val head = growthHead.toLongOrNull()
                    Button(
                        enabled = (weight != null || length != null || head != null) &&
                            (growthWeight.isBlank() || (weight != null && weight in 1L..100_000L)) &&
                            (growthLength.isBlank() || (length != null && length in 1L..2_500L)) &&
                            (growthHead.isBlank() || (head != null && head in 1L..1_000L)),
                        onClick = {
                            val savedWeight = growthWeight
                            val savedLength = growthLength
                            val savedHead = growthHead
                            val chosenAt = logAtMs
                            val at = logTime()
                            scope.launch {
                                runCatching { withContext(Dispatchers.IO) {
                                    if (activeShared) sharing.logGrowthMeasurements(family, child.id, weight, length, head, at)
                                    else store.logGrowthMeasurements(family, child.id, weight, length, head, at)
                                } }.onSuccess {
                                    if (growthWeight == savedWeight) growthWeight = ""
                                    if (growthLength == savedLength) growthLength = ""
                                    if (growthHead == savedHead) growthHead = ""
                                    version++
                                    message = null
                                    resetLogTime(chosenAt)
                                }.onFailure { message = errorText }
                            }
                        },
                    ) { Text(stringResource(R.string.save_growth)) }
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
                            onValueChange = { temperatureEntered = it.take(16) },
                            label = { Text(stringResource(if (temperatureUnit == 30u.toUByte())
                                R.string.temperature_c else R.string.temperature_f)) },
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                            modifier = Modifier.weight(1f),
                            singleLine = true,
                        )
                        Button(enabled = temperatureEntered.isNotBlank(), onClick = {
                            val entered = temperatureEntered.trim()
                            val unit = temperatureUnit
                            val chosenAt = logAtMs
                            val at = logTime()
                            scope.launch {
                                runCatching { withContext(Dispatchers.IO) {
                                    if (activeShared) sharing.logTemperatureEntered(family, child.id, entered, unit, at)
                                    else store.logTemperatureEntered(family, child.id, entered, unit, at)
                                } }.onSuccess {
                                    if (temperatureEntered.trim() == entered) temperatureEntered = ""
                                    version++
                                    message = null
                                    resetLogTime(chosenAt)
                                }.onFailure { message = errorText }
                            }
                        }) { Text(stringResource(R.string.save_temperature)) }
                    }
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
                                }.onFailure { message = errorText }
                            }
                        },
                    ) { Text(stringResource(R.string.save_medication)) }
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
                            }.onFailure { message = errorText }
                        }
                    }) { Text(stringResource(R.string.save_note)) }
                    Text(stringResource(R.string.timeline),
                        modifier = Modifier.onGloballyPositioned {
                            timelineTop = it.positionInParent().y.roundToInt()
                        },
                        style = MaterialTheme.typography.titleLarge)
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
                    val visibleEntries = entries.filter { timelineFilter.includes(it.kind) }
                    if (visibleEntries.isEmpty()) Text(stringResource(
                        if (entries.isEmpty()) R.string.no_entries else R.string.no_matching_entries))
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
                                entry.bottleEntered ?: entry.bottleMl.toString(),
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
                            entry.note != null -> stringResource(R.string.note_entry, entry.note!!)
                            entry.kind == "growth" -> {
                                val parts = listOfNotNull(
                                    entry.growthWeightG?.let { "$it g" },
                                    entry.growthLengthMm?.let { "$it mm" },
                                    entry.growthHeadMm?.let { context.getString(R.string.growth_head_part, it) },
                                )
                                if (parts.isEmpty()) entry.kind
                                else stringResource(R.string.growth_summary, parts.joinToString(" · "))
                            }
                            entry.kind == "temperature" && entry.temperatureC != null ->
                                stringResource(R.string.temperature_entry,
                                    entry.temperatureEntered ?: entry.temperatureC!!,
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
                            else -> entry.kind
                        }
                        Card(Modifier.fillMaxWidth()) {
                            Column(Modifier.padding(12.dp)) {
                                Text(label, fontWeight = FontWeight.SemiBold)
                                Text(DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(entry.startUtcMs)))
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
                                if (entry.kind == "note" && entry.note != null) {
                                    OutlinedButton(onClick = {
                                        pendingNoteEdit = PendingNoteEdit(
                                            family,
                                            entry.childId.copyOf(),
                                            entry.id.copyOf(),
                                            activeShared,
                                            entry.note!!,
                                        )
                                    }) { Text(stringResource(R.string.edit_note)) }
                                }
                                if (entry.kind == "feed.bottle" && entry.bottleMl != null) {
                                    OutlinedButton(onClick = {
                                        pendingBottleEdit = PendingBottleEdit(
                                            family,
                                            entry.childId.copyOf(),
                                            entry.id.copyOf(),
                                            activeShared,
                                            entry.bottleEntered ?: entry.bottleMl.toString(),
                                            entry.bottleUnit ?: 1u.toUByte(),
                                            entry.bottleContent ?: 4u.toUByte(),
                                        )
                                    }) { Text(stringResource(R.string.edit_bottle)) }
                                }
                                if (entry.kind == "feed.breast" && entry.breastSegments?.all {
                                    (it.endUtcMs - it.startUtcMs) % 60_000L == 0L
                                } == true) {
                                    OutlinedButton(onClick = {
                                        pendingBreastEdit = PendingBreastEdit(
                                            family, entry.childId.copyOf(), entry.id.copyOf(), activeShared,
                                            entry.startUtcMs, entry.offsetMinutes,
                                            entry.breastSegments!!.map {
                                                it.side to ((it.endUtcMs - it.startUtcMs) / 60_000L).toString()
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
                                            entry.growthWeightG?.toString().orEmpty(),
                                            entry.growthLengthMm?.toString().orEmpty(),
                                            entry.growthHeadMm?.toString().orEmpty(),
                                        )
                                    }) { Text(stringResource(R.string.edit_growth)) }
                                }
                                if (entry.kind == "temperature" && entry.temperatureC != null) {
                                    OutlinedButton(onClick = {
                                        pendingTemperatureEdit = PendingTemperatureEdit(
                                            family, entry.childId.copyOf(), entry.id.copyOf(), activeShared,
                                            entry.temperatureEntered ?: entry.temperatureC!!,
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

                Spacer(Modifier.height(8.dp))
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
            message?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        }
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
                    change {
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
    pendingNoteEdit?.let { target ->
        AlertDialog(
            onDismissRequest = { pendingNoteEdit = null },
            title = { Text(stringResource(R.string.edit_note)) },
            text = {
                OutlinedTextField(
                    value = target.text,
                    onValueChange = { pendingNoteEdit = target.copy(text = it) },
                    label = { Text(stringResource(R.string.note_text)) },
                )
            },
            confirmButton = {
                Button(enabled = target.text.trim().isNotEmpty(), onClick = {
                    pendingNoteEdit = null
                    val savedAtMs = System.currentTimeMillis()
                    change {
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
                        onValueChange = { pendingBottleEdit = target.copy(amount = it.filter { char ->
                            char.isDigit() || (target.unit != 1u.toUByte() && char == '.')
                        }.take(16)) },
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
                            target.amount, target.unit, target.content, savedAtMs,
                        ) else store.editBottleEntered(
                            target.family, target.childId, target.activityId,
                            target.amount, target.unit, target.content, savedAtMs,
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
                        OutlinedButton(onClick = { pendingBreastEdit = target.copy(segments = target.segments.dropLast(1)) }) {
                            Text(stringResource(R.string.remove_last_segment))
                        }
                    }
                    if (target.segments.size < 8) {
                        OutlinedButton(onClick = {
                            val nextSide = if (target.segments.last().first == 1u.toUByte()) 2u.toUByte() else 1u.toUByte()
                            pendingBreastEdit = target.copy(segments = target.segments + (nextSide to "5"))
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
                Column {
                    OutlinedTextField(
                        value = target.weight,
                        onValueChange = { pendingGrowthEdit = target.copy(weight = it.filter(Char::isDigit).take(6)) },
                        label = { Text(stringResource(R.string.weight_g)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                        singleLine = true,
                    )
                    OutlinedTextField(
                        value = target.length,
                        onValueChange = { pendingGrowthEdit = target.copy(length = it.filter(Char::isDigit).take(4)) },
                        label = { Text(stringResource(R.string.length_mm)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                        singleLine = true,
                    )
                    OutlinedTextField(
                        value = target.head,
                        onValueChange = { pendingGrowthEdit = target.copy(head = it.filter(Char::isDigit).take(4)) },
                        label = { Text(stringResource(R.string.head_mm)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                        singleLine = true,
                    )
                    Text(stringResource(R.string.growth_edit_hint))
                }
            },
            confirmButton = {
                val weight = target.weight.toLongOrNull()
                val length = target.length.toLongOrNull()
                val head = target.head.toLongOrNull()
                Button(enabled = (weight != null || length != null || head != null) &&
                    (target.weight.isBlank() || (weight != null && weight in 1L..100_000L)) &&
                    (target.length.isBlank() || (length != null && length in 1L..2_500L)) &&
                    (target.head.isBlank() || (head != null && head in 1L..1_000L)), onClick = {
                    pendingGrowthEdit = null
                    val savedAtMs = System.currentTimeMillis()
                    change {
                        if (target.shared) sharing.editGrowthMeasurements(
                            target.family, target.childId, target.activityId, weight, length, head, savedAtMs,
                        ) else store.editGrowthMeasurements(
                            target.family, target.childId, target.activityId, weight, length, head, savedAtMs,
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
    pendingChildMetadataEdit?.let { target ->
        AlertDialog(
            onDismissRequest = { pendingChildMetadataEdit = null },
            title = { Text(stringResource(R.string.edit_child_growth_details)) },
            text = {
                Column {
                    OutlinedTextField(
                        value = target.birthDate,
                        onValueChange = { pendingChildMetadataEdit = target.copy(birthDate = it.take(10)) },
                        label = { Text(stringResource(R.string.birth_date)) },
                        singleLine = true,
                    )
                    Text(stringResource(R.string.birth_date_edit_hint))
                    Text(stringResource(R.string.growth_chart_sex))
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        listOf(
                            1u.toUByte() to R.string.sex_female,
                            2u.toUByte() to R.string.sex_male,
                            3u.toUByte() to R.string.sex_unspecified,
                        ).forEach { (code, label) ->
                            FilterChip(
                                selected = target.sex == code,
                                onClick = { pendingChildMetadataEdit = target.copy(sex = code) },
                                label = { Text(stringResource(label)) },
                            )
                        }
                    }
                }
            },
            confirmButton = {
                Button(onClick = {
                    val birthDay = runCatching {
                        target.birthDate.takeIf { it.isNotBlank() }?.let { LocalDate.parse(it).toEpochDay() }
                    }.getOrElse { message = context.getString(R.string.birth_date_invalid); return@Button }
                    change(onSaved = { pendingChildMetadataEdit = null }) {
                        if (target.shared) sharing.editChildMetadata(
                            target.family, target.childId, birthDay, target.sex, System.currentTimeMillis(),
                        ) else store.editChildMetadata(
                            target.family, target.childId, birthDay, target.sex, System.currentTimeMillis(),
                        )
                    }
                }) { Text(stringResource(R.string.save_changes)) }
            },
            dismissButton = {
                OutlinedButton(onClick = { pendingChildMetadataEdit = null }) { Text(stringResource(R.string.cancel)) }
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
                        onValueChange = { pendingTemperatureEdit = target.copy(entered = it.take(16)) },
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
                Button(enabled = target.entered.isNotBlank(), onClick = {
                    val savedAtMs = System.currentTimeMillis()
                    change(onSaved = { pendingTemperatureEdit = null }) {
                        if (target.shared) sharing.editTemperatureEntered(
                            target.family, target.childId, target.activityId,
                            target.entered, target.unit, savedAtMs,
                        ) else store.editTemperatureEntered(
                            target.family, target.childId, target.activityId,
                            target.entered, target.unit, savedAtMs,
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
    if (snapshot.unsentCount > 0uL) {
        Text(stringResource(R.string.shared_pending_changes, snapshot.unsentCount.toLong()))
    }
    if (snapshot.inertCount > 0uL) {
        Text(
            stringResource(R.string.shared_unreadable_batches, snapshot.inertCount.toLong()),
            color = MaterialTheme.colorScheme.error,
        )
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
