package org.babytrack.app

import android.Manifest
import android.os.Bundle
import android.os.Build
import android.app.ActivityManager
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
import androidx.compose.material3.Text
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
import androidx.compose.ui.text.font.FontWeight
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
import uniffi.babytrack_core_ffi.RestoredOriginRow
import uniffi.babytrack_core_ffi.SharedSnapshotRow
import uniffi.babytrack_core_ffi.SharedSyncRow
import java.text.DateFormat
import java.time.LocalDate
import java.util.Date
import java.util.TimeZone

class MainActivity : ComponentActivity() {
    private var incomingInvitation by mutableStateOf<String?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        incomingInvitation = invitationFrom(intent)
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
        incomingInvitation = invitationFrom(intent)
    }
}

private fun invitationFrom(intent: Intent?): String? {
    if (intent?.action != Intent.ACTION_SEND || intent.type != "text/plain") return null
    val fragment = intent.getStringExtra(Intent.EXTRA_TEXT)?.trim() ?: return null
    return fragment.takeIf { it.length <= 2048 && it.startsWith("#bt-invite=v1.") }
}

private fun ByteArray.key(): String = joinToString("") { "%02x".format(it) }

private fun deviceLabelKey(familyId: ByteArray, deviceId: ByteArray): String =
    familyId.key() + ":" + deviceId.key()

private data class CompletedSave(val atMs: Long, val revision: ULong)
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
)
private data class PendingSleepEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val startUtcMs: Long,
    val minutes: String,
)
private data class PendingTemperatureEdit(
    val family: FamilyRef,
    val childId: ByteArray,
    val activityId: ByteArray,
    val shared: Boolean,
    val enteredC: String,
)
internal data class ScreenData(
    val families: List<FamilyRef>,
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
    val joinedSnapshot: SharedSnapshotRow?,
    val activeSleepCount: Int,
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
    val recipients = sharing.recipientFamilies()
    val recipient = recipients.find { it.familyId.key() == selectedRecipient } ?: recipients.firstOrNull()
    val readyJoined = recipients.mapNotNull { candidate ->
        runCatching { candidate to sharing.snapshot(candidate) }.getOrNull()
    }
    val familyChildNames = mutableMapOf<String, String>()
    for (candidate in local) {
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
    val shown = local + readyJoined.map { it.first }
    val family = shown.find { it.familyId.key() == selectedFamily } ?: shown.firstOrNull()
    val localFamily = family != null && local.any { it.familyId.key() == family.familyId.key() }
    val recipientSnapshot = readyJoined.find { it.first.familyId.key() == family?.familyId?.key() }?.second
    val shared = recipientSnapshot != null || (family?.let(sharing::isShared) ?: false)
    val snapshot = recipientSnapshot ?: if (shared) sharing.snapshot(family ?: error("Shared Family absent")) else null
    val kids = snapshot?.children ?: family?.let(store::children).orEmpty()
    val child = kids.find { it.id.key() == selectedChild } ?: kids.firstOrNull()
    val history = if (family != null && child != null) {
        snapshot?.activities?.filter { it.childId.contentEquals(child.id) }
            ?: store.timeline(family, child.id)
    } else emptyList()
    return ScreenData(
        shown, familyChildNames, family?.familyId?.key(), localFamily, kids, history,
        if (!shared) family?.let(store::revision) ?: 0uL else 0uL,
        if (!shared) family?.let(store::restoredOrigin) else null,
        shared, snapshot, recipients, joinedSnapshot,
        runningSleepCount(store, sharing, local, recipients),
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TrackerScreen(
    store: NativeLocalStore,
    sharing: ShareCoordinator,
    incomingInvitation: String?,
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
    var familyChildNames by remember { mutableStateOf<Map<String, String>>(emptyMap()) }
    var children by remember { mutableStateOf<List<ChildRow>>(emptyList()) }
    var entries by remember { mutableStateOf<List<ActivityRow>>(emptyList()) }
    var revision by remember { mutableStateOf(0uL) }
    var restoredOrigin by remember { mutableStateOf<RestoredOriginRow?>(null) }
    var isShared by remember { mutableStateOf(false) }
    var activeSharedSnapshot by remember { mutableStateOf<SharedSnapshotRow?>(null) }
    var loadedFamilyKey by remember { mutableStateOf<String?>(null) }
    var activeFamilyIsLocal by remember { mutableStateOf(false) }
    var saveStatusVersion by remember { mutableStateOf(0) }
    var selectedFamily by remember { mutableStateOf<String?>(null) }
    var selectedChild by remember { mutableStateOf<String?>(null) }
    var childName by remember { mutableStateOf("") }
    var childRename by remember { mutableStateOf<String?>(null) }
    var childBirthDate by remember { mutableStateOf("") }
    var childSex by remember { mutableStateOf(3u.toUByte()) }
    var amount by remember { mutableStateOf("") }
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
    var noteText by remember { mutableStateOf("") }
    var growthWeight by remember { mutableStateOf("") }
    var growthLength by remember { mutableStateOf("") }
    var temperatureC by remember { mutableStateOf("") }
    var medicationName by remember { mutableStateOf("") }
    var doseAmount by remember { mutableStateOf("") }
    var doseUnit by remember { mutableStateOf("") }
    var message by remember { mutableStateOf<String?>(null) }
    var automaticSyncDelayed by remember { mutableStateOf(false) }
    var automaticSyncBlocked by remember { mutableStateOf(false) }
    var removalTarget by remember { mutableStateOf<ByteArray?>(null) }
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
    var pendingDiaperEdit by remember { mutableStateOf<PendingDiaperEdit?>(null) }
    var pendingSolidsEdit by remember { mutableStateOf<PendingSolidsEdit?>(null) }
    var pendingGrowthEdit by remember { mutableStateOf<PendingGrowthEdit?>(null) }
    var pendingSleepEdit by remember { mutableStateOf<PendingSleepEdit?>(null) }
    var pendingTemperatureEdit by remember { mutableStateOf<PendingTemperatureEdit?>(null) }
    var relayOrigin by remember { mutableStateOf("") }
    var relayPublicKey by remember { mutableStateOf("") }
    var shareStage by remember { mutableStateOf<String?>(null) }
    var invitationFragment by remember { mutableStateOf<String?>(null) }
    var receivedFragment by remember { mutableStateOf("") }
    var showJoinForm by remember { mutableStateOf(false) }
    var showShareForm by remember { mutableStateOf(false) }
    var joinStage by remember { mutableStateOf<String?>(null) }
    var sharedSnapshot by remember { mutableStateOf<SharedSnapshotRow?>(null) }
    var sharedSelectedChild by remember { mutableStateOf<String?>(null) }
    var recipientFamilies by remember { mutableStateOf<List<FamilyRef>>(emptyList()) }
    var selectedRecipient by remember { mutableStateOf<String?>(null) }
    var sharedChildName by remember { mutableStateOf("") }
    var inviteAsManager by remember { mutableStateOf(false) }
    LaunchedEffect(incomingInvitation) {
        if (incomingInvitation != null) {
            receivedFragment = incomingInvitation
            selectedRecipient = null
            showJoinForm = true
        }
    }
    val errorText = stringResource(R.string.error)
    val savedText = stringResource(R.string.saved)
    val restoredText = stringResource(R.string.restored)
    LaunchedEffect(selectedFamily) {
        val family = families.find { it.familyId.key() == selectedFamily }
        relayOrigin = family?.let {
            if (recipientFamilies.any { recipient -> recipient.familyId.key() == it.familyId.key() }) {
                runCatching { sharing.recipientOrigin(it) }.getOrNull()
            } else lastRelayOrigin(it)
        }.orEmpty()
    }
    LaunchedEffect(selectedFamily, selectedChild) {
        breastDraftSegments = emptyList()
        breastMinutes = ""
        childRename = null
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
    LaunchedEffect(foreground, selectedRecipient) {
        if (foreground) while (isActive) {
            val (syncResult, terminalReasons) = withContext(Dispatchers.IO) {
                var failed = false
                var blocked = false
                val stages = mutableMapOf<String, uniffi.babytrack_core_ffi.RecipientSyncRow>()
                val terminals = mutableMapOf<String, InvitationTerminalReason>()
                for (family in store.families()) {
                    val origin = lastRelayOrigin(family)
                    if (origin != null && sharing.isShared(family)) {
                        runCatching { sharing.advanceManager(family, origin) }
                            .onFailure { failed = true; if (it is SharedUploadBlocked) blocked = true }
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
                Triple(failed, blocked, stages) to terminals
            }
            automaticSyncDelayed = syncResult.first
            automaticSyncBlocked = syncResult.second
            val recipientStages = syncResult.third
            terminalReasons[selectedRecipient]?.let { reason ->
                joinStage = terminalInvitationMessage(context, reason)
                showJoinForm = true
                sharedSnapshot = null
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
            runCatching {
                val bytes = withContext(Dispatchers.IO) { readFile(uri) ?: error("Missing backup") }
                pendingRestore = bytes
                restoreInfo = null
                restorePassword = ""
                pendingRestoreProtected = bytes.size >= 5 && bytes.copyOfRange(0, 5).contentEquals("BTBK1".toByteArray())
                if (pendingRestoreProtected) {
                    message = passwordNeeded
                } else {
                    restoreInfo = withContext(Dispatchers.IO) { store.inspectReadable(bytes) }
                    message = null
                }
            }.onFailure { message = if (it is BackupTooLarge) tooLargeError else errorText }
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
            loadedFamilyKey = data.activeFamilyKey
            recipientFamilies = data.recipients
            selectedRecipient = data.recipients.find { it.familyId.key() == selectedRecipient }
                ?.familyId?.key() ?: data.recipients.firstOrNull()?.familyId?.key()
            sharedSnapshot = data.joinedSnapshot
            runCatching { SleepTimerNotifications.update(context, data.activeSleepCount) }
                .onFailure { android.util.Log.w("BabytrackTimer", "Could not update sleep notification", it) }
        }.onFailure { message = errorText }
    }
    val family = families.find { it.familyId.key() == selectedFamily }
    val child = children.find { it.id.key() == selectedChild }
    val activeShared = isShared && loadedFamilyKey == selectedFamily
    val completed = remember(selectedFamily, saveStatusVersion) { family?.let(lastSave) }
    val filename = stringResource(R.string.backup_filename)
    val protectedFilename = stringResource(R.string.protected_backup_filename)

    Scaffold(topBar = { TopAppBar(title = { Text(stringResource(R.string.screen_title)) }) }) { padding ->
        Column(
            modifier = Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
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
                            FilterChip(
                                selected = recipient.familyId.key() == selectedRecipient,
                                onClick = { selectedRecipient = recipient.familyId.key() },
                                label = { Text(stringResource(R.string.joined_family_number, index + 1)) },
                            )
                        }
                        OutlinedTextField(
                            value = receivedFragment,
                            onValueChange = { receivedFragment = it },
                            label = { Text(stringResource(R.string.received_fragment)) },
                            modifier = Modifier.fillMaxWidth(),
                        )
                        Button(enabled = selectedRecipient != null || receivedFragment.isNotBlank(), onClick = {
                            joinStage = context.getString(R.string.join_preparing)
                            scope.launch {
                                runCatching {
                                    withContext(Dispatchers.IO) {
                                        val recipient = recipientFamilies.find { it.familyId.key() == selectedRecipient }
                                        if (receivedFragment.isNotBlank()) sharing.claim(receivedFragment.trim())
                                        else sharing.retryClaim(recipient ?: error("No saved recipient claim"))
                                    }
                                }.onSuccess { prepared ->
                                    selectedRecipient = prepared.family.familyId.key()
                                    version++
                                    joinStage = context.getString(R.string.join_pending)
                                    message = null
                                }.onFailure { failure ->
                                    joinStage = (failure as? InvitationTerminal)?.reason
                                        ?.let { terminalInvitationMessage(context, it) }
                                        ?: context.getString(R.string.join_retry)
                                    message = if (failure is InvitationTerminal) null else errorText
                                }
                            }
                        }) { Text(stringResource(R.string.join_or_retry)) }
                        OutlinedButton(enabled = selectedRecipient != null || receivedFragment.isNotBlank(), onClick = {
                            joinStage = context.getString(R.string.proof_preparing)
                            scope.launch {
                                runCatching {
                                    withContext(Dispatchers.IO) {
                                        val recipient = recipientFamilies.find { it.familyId.key() == selectedRecipient }
                                        if (recipient != null) sharing.proveChallenge(recipient)
                                        else sharing.proveChallenge(receivedFragment.trim())
                                    }
                                }.onSuccess {
                                    joinStage = context.getString(R.string.proof_confirmed)
                                    message = null
                                }.onFailure {
                                    joinStage = context.getString(R.string.join_retry)
                                    message = errorText
                                }
                            }
                        }) { Text(stringResource(R.string.prove_challenge)) }
                        OutlinedButton(enabled = selectedRecipient != null || receivedFragment.isNotBlank(), onClick = {
                            joinStage = context.getString(R.string.history_loading)
                            scope.launch {
                                runCatching {
                                    withContext(Dispatchers.IO) {
                                        val recipient = recipientFamilies.find { it.familyId.key() == selectedRecipient }
                                        val progress = if (recipient != null) sharing.syncRecipient(recipient)
                                            else sharing.syncRecipient(receivedFragment.trim())
                                        progress to if (progress.ready) {
                                            if (recipient != null) sharing.snapshot(recipient)
                                            else sharing.snapshotForFragment(receivedFragment.trim())
                                        } else null
                                    }
                                }.onSuccess { (progress, snapshot) ->
                                    sharedSnapshot = snapshot
                                    joinStage = if (progress.removed) {
                                        removedHistoryMessage(context, progress)
                                    } else if (progress.ready) {
                                        context.getString(R.string.history_ready, progress.childCount.toLong())
                                    } else if (progress.awaitingGrant) {
                                        pendingRecipientMessage(context, progress)
                                    } else {
                                        context.getString(R.string.history_pending, progress.verifiedCursor.toLong())
                                    }
                                    message = null
                                }.onFailure {
                                    joinStage = context.getString(R.string.join_retry)
                                    message = errorText
                                }
                            }
                        }) { Text(stringResource(R.string.load_shared_history)) }
                        joinStage?.let { Text(it) }
                        sharedSnapshot?.let { snapshot ->
                            Text(stringResource(R.string.shared_children), style = MaterialTheme.typography.titleMedium)
                            Text(stringResource(R.string.shared_manual_sync))
                            SharedHealth(snapshot, deviceLabels, onNameDevice = { target ->
                                val key = deviceLabelKey(snapshot.family.familyId, target)
                                deviceLabelTarget = key
                                deviceLabelDraft = deviceLabels[key].orEmpty()
                            })
                            if (snapshot.family.familyId.key() == selectedRecipient &&
                                sharing.isAdmittedManager(snapshot.family)) {
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
                                            putExtra(Intent.EXTRA_TEXT, fragment)
                                        }
                                        context.startActivity(Intent.createChooser(
                                            send, context.getString(R.string.share_invitation),
                                        ))
                                    }) { Text(stringResource(R.string.share_invitation)) }
                                }
                            }
                            OutlinedButton(onClick = {
                                scope.launch {
                                    runCatching { withContext(Dispatchers.IO) {
                                        sharing.privateCopy(snapshot.family, System.currentTimeMillis())
                                    } }.onSuccess { copy ->
                                        selectedFamily = copy.familyId.key()
                                        selectedChild = null
                                        version++
                                        message = context.getString(R.string.private_copy_created)
                                    }.onFailure { message = errorText }
                                }
                            }) { Text(stringResource(R.string.make_private_copy)) }
                            Text(stringResource(R.string.shared_backup_description))
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
                            OutlinedButton(
                                enabled = !protectBackup || backupPassword.isNotEmpty(),
                                onClick = {
                                    scope.launch {
                                        val protected = protectBackup
                                        val password = backupPassword
                                        runCatching { withContext(Dispatchers.IO) {
                                            sharing.backupFile(
                                                snapshot.family, System.currentTimeMillis(),
                                                if (protected) password else null,
                                                availableMemory().toULong(),
                                            )
                                        } }.onSuccess {
                                            pendingBackup = it
                                            backupPassword = ""
                                            saveLauncher.launch(if (protected) protectedFilename else filename)
                                        }.onFailure { message = errorText }
                                    }
                                },
                            ) { Text(stringResource(R.string.save_backup)) }
                            OutlinedButton(onClick = {
                                scope.launch {
                                    runCatching {
                                        withContext(Dispatchers.IO) {
                                            val progress = sharing.syncRecipientAndUpload(snapshot.family)
                                            progress to sharing.snapshot(snapshot.family)
                                        }
                                    }.onSuccess { (progress, updated) ->
                                        sharedSnapshot = updated
                                        joinStage = sharedSyncMessage(context, progress)
                                        message = null
                                    }.onFailure { message = if (it is SharedUploadBlocked) context.getString(R.string.shared_upload_blocked) else errorText }
                                }
                            }) { Text(stringResource(R.string.sync_shared)) }
                            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                OutlinedTextField(
                                    value = sharedChildName,
                                    onValueChange = { sharedChildName = it },
                                    label = { Text(stringResource(R.string.child_name)) },
                                    modifier = Modifier.weight(1f),
                                )
                                Button(enabled = sharedChildName.isNotBlank(), onClick = {
                                    val name = sharedChildName.trim()
                                    scope.launch {
                                        runCatching {
                                            withContext(Dispatchers.IO) {
                                                sharing.addChild(snapshot.family, name, System.currentTimeMillis())
                                                sharing.snapshot(snapshot.family)
                                            }
                                        }.onSuccess { updated ->
                                            sharedSnapshot = updated
                                            sharedChildName = ""
                                            message = null
                                        }.onFailure { message = errorText }
                                    }
                                }) { Text(stringResource(R.string.add_child)) }
                            }
                            snapshot.children.forEach { item ->
                                FilterChip(
                                    selected = item.id.key() == sharedSelectedChild,
                                    onClick = { sharedSelectedChild = item.id.key() },
                                    label = { Text(item.name) },
                                )
                            }
                            val target = sharedSelectedChild ?: snapshot.children.firstOrNull()?.id?.key()
                            snapshot.children.find { it.id.key() == target }?.let { child ->
                                Button(onClick = {
                                    scope.launch {
                                        runCatching {
                                            withContext(Dispatchers.IO) {
                                                sharing.logDiaper(snapshot.family, child.id, 1u.toUByte(), nowTime())
                                                sharing.snapshot(snapshot.family)
                                            }
                                        }.onSuccess { sharedSnapshot = it; message = null }
                                            .onFailure { message = errorText }
                                    }
                                }) { Text(stringResource(R.string.wet)) }
                            }
                            snapshot.activities.filter { it.childId.key() == target }.forEach { entry ->
                                Card(Modifier.fillMaxWidth()) {
                                    Text(
                                        stringResource(
                                            R.string.shared_entry,
                                            entry.kind,
                                            DateFormat.getDateTimeInstance().format(Date(entry.startUtcMs)),
                                        ),
                                        modifier = Modifier.padding(12.dp),
                                    )
                                }
                            }
                        }
                    }
                }
            }

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
                                        putExtra(Intent.EXTRA_TEXT, fragment)
                                    }
                                    context.startActivity(Intent.createChooser(
                                        send,
                                        context.getString(R.string.share_invitation),
                                    ))
                                }) { Text(stringResource(R.string.share_invitation)) }
                            }
                            OutlinedButton(onClick = {
                                shareStage = context.getString(R.string.challenge_preparing)
                                scope.launch {
                                    runCatching {
                                        withContext(Dispatchers.IO) {
                                            sharing.respondToClaim(family, relayOrigin.trim())
                                        }
                                    }.onSuccess {
                                        shareStage = context.getString(R.string.challenge_confirmed)
                                        message = null
                                    }.onFailure {
                                        shareStage = context.getString(R.string.share_retry)
                                        message = errorText
                                    }
                                }
                            }) { Text(stringResource(R.string.respond_to_claim)) }
                            OutlinedButton(onClick = {
                                shareStage = context.getString(R.string.admission_preparing)
                                scope.launch {
                                    runCatching {
                                        withContext(Dispatchers.IO) {
                                            sharing.admitProvedDevice(family, relayOrigin.trim())
                                        }
                                    }.onSuccess {
                                        shareStage = context.getString(R.string.admission_confirmed)
                                        message = null
                                    }.onFailure {
                                        shareStage = context.getString(R.string.share_retry)
                                        message = errorText
                                    }
                                }
                            }) { Text(stringResource(R.string.admit_device)) }
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
                }
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

                if (child != null) {
                    Text(stringResource(R.string.log_diaper), style = MaterialTheme.typography.titleLarge)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        listOf(1u.toUByte() to R.string.wet, 2u.toUByte() to R.string.dirty, 3u.toUByte() to R.string.both).forEach { (kind, label) ->
                            Button(onClick = { change {
                                if (activeShared) sharing.logDiaper(family, child.id, kind, nowTime())
                                else store.logDiaper(family, child.id, kind, nowTime())
                            } }) {
                                Text(stringResource(label))
                            }
                        }
                    }
                    Text(stringResource(R.string.log_bottle), style = MaterialTheme.typography.titleLarge)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedTextField(
                            value = amount,
                            onValueChange = { amount = it.filter(Char::isDigit) },
                            label = { Text(stringResource(R.string.amount_ml)) },
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                            modifier = Modifier.weight(1f),
                            singleLine = true,
                        )
                        Button(enabled = (amount.toLongOrNull() ?: 0) > 0, onClick = {
                            val ml = amount.toLongOrNull() ?: return@Button
                            change {
                                if (activeShared) sharing.logBottleMl(family, child.id, ml, nowTime())
                                else store.logBottleMl(family, child.id, ml, 2u.toUByte(), nowTime())
                            }
                            amount = ""
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
                        val end = nowTime()
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
                        val minutes = pumpMinutes.toLongOrNull() ?: return@Button
                        val left = pumpLeft.toLongOrNull()
                        val right = pumpRight.toLongOrNull()
                        val total = pumpTotal.toLongOrNull()
                        val input = PumpInput(left, right, total)
                        val end = nowTime()
                        val interval = ActivityWhen(end.startUtcMs - minutes * 60_000L,
                            end.offsetMinutes, end.savedAtMs)
                        scope.launch {
                            runCatching { withContext(Dispatchers.IO) {
                                if (activeShared) sharing.logPump(family, child.id, input, interval, end.startUtcMs)
                                else store.logPump(family, child.id, input, interval, end.startUtcMs)
                            } }.onSuccess {
                                pumpMinutes = ""
                                pumpLeft = ""
                                pumpRight = ""
                                pumpTotal = ""
                                version++
                                message = null
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
                        val foods = solidsFoods.lines().map { it.trim() }.filter { it.isNotEmpty() }
                        val eaten = solidsAmount.trim()
                        scope.launch {
                            runCatching { withContext(Dispatchers.IO) {
                                if (activeShared) sharing.logSolids(family, child.id, foods, eaten, nowTime())
                                else store.logSolids(family, child.id, foods, eaten, nowTime())
                            } }.onSuccess {
                                solidsFoods = ""
                                solidsAmount = ""
                                version++
                                message = null
                            }.onFailure { message = errorText }
                        }
                    }) { Text(stringResource(R.string.save_solids)) }
                    Text(stringResource(R.string.log_sleep), style = MaterialTheme.typography.titleLarge)
                    Button(onClick = {
                        change(onSaved = {
                            if (Build.VERSION.SDK_INT >= 33 &&
                                context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
                            ) notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
                        }) {
                            if (activeShared) sharing.startSleep(family, child.id, nowTime())
                            else store.startSleep(family, child.id, nowTime())
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
                            val duration = sleepMinutes.toLongOrNull() ?: return@Button
                            val end = System.currentTimeMillis()
                            val start = end - duration * 60_000
                            val zone = TimeZone.getDefault()
                            val whenStarted = ActivityWhen(start, (zone.getOffset(start) / 60_000).toShort(), end)
                            val endOffset = (zone.getOffset(end) / 60_000).toShort()
                            change {
                                if (activeShared) sharing.logSleep(family, child.id, whenStarted, end, endOffset)
                                else store.logSleep(family, child.id, whenStarted, end, endOffset)
                            }
                            sleepMinutes = ""
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
                    val weight = growthWeight.toLongOrNull()
                    val length = growthLength.toLongOrNull()
                    Button(
                        enabled = (weight != null || length != null) &&
                            (growthWeight.isBlank() || (weight != null && weight in 1L..100_000L)) &&
                            (growthLength.isBlank() || (length != null && length in 1L..2_500L)),
                        onClick = {
                            val savedWeight = growthWeight
                            val savedLength = growthLength
                            scope.launch {
                                runCatching { withContext(Dispatchers.IO) {
                                    if (activeShared) sharing.logGrowth(family, child.id, weight, length, nowTime())
                                    else store.logGrowth(family, child.id, weight, length, nowTime())
                                } }.onSuccess {
                                    if (growthWeight == savedWeight) growthWeight = ""
                                    if (growthLength == savedLength) growthLength = ""
                                    version++
                                    message = null
                                }.onFailure { message = errorText }
                            }
                        },
                    ) { Text(stringResource(R.string.save_growth)) }
                    Text(stringResource(R.string.log_temperature), style = MaterialTheme.typography.titleLarge)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedTextField(
                            value = temperatureC,
                            onValueChange = { temperatureC = it.take(16) },
                            label = { Text(stringResource(R.string.temperature_c)) },
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                            modifier = Modifier.weight(1f),
                            singleLine = true,
                        )
                        Button(enabled = temperatureC.isNotBlank(), onClick = {
                            val entered = temperatureC.trim()
                            scope.launch {
                                runCatching { withContext(Dispatchers.IO) {
                                    if (activeShared) sharing.logTemperatureC(family, child.id, entered, nowTime())
                                    else store.logTemperatureC(family, child.id, entered, nowTime())
                                } }.onSuccess {
                                    if (temperatureC.trim() == entered) temperatureC = ""
                                    version++
                                    message = null
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
                            scope.launch {
                                runCatching { withContext(Dispatchers.IO) {
                                    if (activeShared) sharing.logMedication(family, child.id, input, nowTime())
                                    else store.logMedication(family, child.id, input, nowTime())
                                } }.onSuccess {
                                    if (medicationName.trim() == name) medicationName = ""
                                    if (doseAmount.trim() == amount) doseAmount = ""
                                    if (doseUnit.trim() == unit) doseUnit = ""
                                    version++
                                    message = null
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
                        scope.launch {
                            runCatching { withContext(Dispatchers.IO) {
                                if (activeShared) sharing.logNote(family, child.id, note, nowTime())
                                else store.logNote(family, child.id, note, nowTime())
                            } }.onSuccess {
                                if (noteText.trim() == note) noteText = ""
                                version++
                                message = null
                            }.onFailure { message = errorText }
                        }
                    }) { Text(stringResource(R.string.save_note)) }
                    Text(stringResource(R.string.timeline), style = MaterialTheme.typography.titleLarge)
                    if (entries.isEmpty()) Text(stringResource(R.string.no_entries))
                    entries.forEach { entry ->
                        val label = when {
                            entry.bottleMl != null -> stringResource(R.string.bottle, entry.bottleMl!!)
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
                            entry.kind == "growth" && entry.growthWeightG != null && entry.growthLengthMm != null ->
                                stringResource(R.string.growth_both, entry.growthWeightG!!, entry.growthLengthMm!!)
                            entry.kind == "growth" && entry.growthWeightG != null ->
                                stringResource(R.string.growth_weight, entry.growthWeightG!!)
                            entry.kind == "growth" && entry.growthLengthMm != null ->
                                stringResource(R.string.growth_length, entry.growthLengthMm!!)
                            entry.kind == "temperature" && entry.temperatureC != null ->
                                stringResource(R.string.temperature_entry, entry.temperatureC!!)
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
                                            entry.bottleMl.toString(),
                                        )
                                    }) { Text(stringResource(R.string.edit_bottle)) }
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
                                if (entry.kind == "growth") {
                                    OutlinedButton(onClick = {
                                        pendingGrowthEdit = PendingGrowthEdit(
                                            family, entry.childId.copyOf(), entry.id.copyOf(), activeShared,
                                            entry.growthWeightG?.toString().orEmpty(),
                                            entry.growthLengthMm?.toString().orEmpty(),
                                        )
                                    }) { Text(stringResource(R.string.edit_growth)) }
                                }
                                if (entry.kind == "temperature" && entry.temperatureC != null) {
                                    OutlinedButton(onClick = {
                                        pendingTemperatureEdit = PendingTemperatureEdit(
                                            family, entry.childId.copyOf(), entry.id.copyOf(), activeShared,
                                            entry.temperatureC!!,
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
                OutlinedTextField(
                    value = target.amount,
                    onValueChange = { pendingBottleEdit = target.copy(amount = it) },
                    label = { Text(stringResource(R.string.amount_ml)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                )
            },
            confirmButton = {
                val amount = target.amount.toLongOrNull()
                Button(enabled = amount != null && amount in 1..1_000_000, onClick = {
                    pendingBottleEdit = null
                    val savedAtMs = System.currentTimeMillis()
                    change {
                        val ml = amount ?: error("Bottle amount missing")
                        if (target.shared) sharing.editBottleMl(
                            target.family, target.childId, target.activityId, ml, savedAtMs,
                        ) else store.editBottleMl(
                            target.family, target.childId, target.activityId, ml, savedAtMs,
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
                    Text(stringResource(R.string.growth_edit_hint))
                }
            },
            confirmButton = {
                val weight = target.weight.toLongOrNull()
                val length = target.length.toLongOrNull()
                Button(enabled = (weight != null || length != null) &&
                    (target.weight.isBlank() || (weight != null && weight in 1L..100_000L)) &&
                    (target.length.isBlank() || (length != null && length in 1L..2_500L)), onClick = {
                    pendingGrowthEdit = null
                    val savedAtMs = System.currentTimeMillis()
                    change {
                        if (target.shared) sharing.editGrowth(
                            target.family, target.childId, target.activityId, weight, length, savedAtMs,
                        ) else store.editGrowth(
                            target.family, target.childId, target.activityId, weight, length, savedAtMs,
                        )
                    }
                }) { Text(stringResource(R.string.save_changes)) }
            },
            dismissButton = {
                OutlinedButton(onClick = { pendingGrowthEdit = null }) { Text(stringResource(R.string.cancel)) }
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
    pendingTemperatureEdit?.let { target ->
        AlertDialog(
            onDismissRequest = { pendingTemperatureEdit = null },
            title = { Text(stringResource(R.string.edit_temperature)) },
            text = {
                OutlinedTextField(
                    value = target.enteredC,
                    onValueChange = { pendingTemperatureEdit = target.copy(enteredC = it.take(16)) },
                    label = { Text(stringResource(R.string.temperature_c)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                    singleLine = true,
                )
            },
            confirmButton = {
                Button(enabled = target.enteredC.isNotBlank(), onClick = {
                    pendingTemperatureEdit = null
                    val savedAtMs = System.currentTimeMillis()
                    change {
                        if (target.shared) sharing.editTemperatureC(
                            target.family, target.childId, target.activityId, target.enteredC, savedAtMs,
                        ) else store.editTemperatureC(
                            target.family, target.childId, target.activityId, target.enteredC, savedAtMs,
                        )
                    }
                }) { Text(stringResource(R.string.save_changes)) }
            },
            dismissButton = {
                OutlinedButton(onClick = { pendingTemperatureEdit = null }) { Text(stringResource(R.string.cancel)) }
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
    val now = System.currentTimeMillis()
    return ActivityWhen(now, (TimeZone.getDefault().getOffset(now) / 60_000).toShort(), now)
}

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
