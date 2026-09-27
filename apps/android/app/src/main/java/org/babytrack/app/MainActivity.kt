package org.babytrack.app

import android.os.Bundle
import android.app.ActivityManager
import android.content.Context
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
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
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
import uniffi.babytrack_core_ffi.ChildRow
import uniffi.babytrack_core_ffi.FamilyRef
import uniffi.babytrack_core_ffi.NativeLocalStore
import uniffi.babytrack_core_ffi.RestoredOriginRow
import uniffi.babytrack_core_ffi.SharedSnapshotRow
import uniffi.babytrack_core_ffi.SharedSyncRow
import java.text.DateFormat
import java.util.Date
import java.util.TimeZone

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        runCatching { SharedSyncJobService.schedule(this) }
            .onFailure { Log.w("BabytrackSync", "Could not schedule periodic shared sync", it) }
        val database = filesDir.resolve("families.db")
        val savedFiles = getSharedPreferences("completed_file_saves", MODE_PRIVATE)
        val relayOrigins = getSharedPreferences("shared_relay_origins", MODE_PRIVATE)
        val availableMemory = {
            ActivityManager.MemoryInfo().also {
                (getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager).getMemoryInfo(it)
            }.availMem
        }
        setContent {
            MaterialTheme {
                TrackerScreen(
                    store = remember { NativeLocalStore.open(database.absolutePath) },
                    sharing = remember { ShareCoordinator(this, database.absolutePath) },
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
}

private fun ByteArray.key(): String = joinToString("") { "%02x".format(it) }

private data class CompletedSave(val atMs: Long, val revision: ULong)
private data class ScreenData(
    val families: List<FamilyRef>,
    val activeFamilyKey: String?,
    val children: List<ChildRow>,
    val entries: List<ActivityRow>,
    val revision: ULong,
    val restoredOrigin: RestoredOriginRow?,
    val shared: Boolean,
    val mainSharedSnapshot: SharedSnapshotRow?,
    val recipients: List<FamilyRef>,
    val joinedSnapshot: SharedSnapshotRow?,
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TrackerScreen(
    store: NativeLocalStore,
    sharing: ShareCoordinator,
    readFile: (android.net.Uri) -> ByteArray?,
    writeFile: (android.net.Uri, ByteArray) -> Unit,
    availableMemory: () -> Long,
    lastSave: (FamilyRef) -> CompletedSave?,
    recordSave: (BackupFileRow) -> Boolean,
    lastRelayOrigin: (FamilyRef) -> String?,
    recordRelayOrigin: (FamilyRef, String) -> Boolean,
) {
    val context = LocalContext.current
    DisposableEffect(store, sharing) { onDispose { store.close(); sharing.close() } }
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
    var families by remember { mutableStateOf<List<FamilyRef>>(emptyList()) }
    var children by remember { mutableStateOf<List<ChildRow>>(emptyList()) }
    var entries by remember { mutableStateOf<List<ActivityRow>>(emptyList()) }
    var revision by remember { mutableStateOf(0uL) }
    var restoredOrigin by remember { mutableStateOf<RestoredOriginRow?>(null) }
    var isShared by remember { mutableStateOf(false) }
    var activeSharedSnapshot by remember { mutableStateOf<SharedSnapshotRow?>(null) }
    var loadedFamilyKey by remember { mutableStateOf<String?>(null) }
    var saveStatusVersion by remember { mutableStateOf(0) }
    var selectedFamily by remember { mutableStateOf<String?>(null) }
    var selectedChild by remember { mutableStateOf<String?>(null) }
    var childName by remember { mutableStateOf("") }
    var amount by remember { mutableStateOf("") }
    var message by remember { mutableStateOf<String?>(null) }
    var automaticSyncDelayed by remember { mutableStateOf(false) }
    var automaticSyncBlocked by remember { mutableStateOf(false) }
    var relayOrigin by remember { mutableStateOf("") }
    var relayPublicKey by remember { mutableStateOf("") }
    var shareStage by remember { mutableStateOf<String?>(null) }
    var invitationFragment by remember { mutableStateOf<String?>(null) }
    var receivedFragment by remember { mutableStateOf("") }
    var joinStage by remember { mutableStateOf<String?>(null) }
    var sharedSnapshot by remember { mutableStateOf<SharedSnapshotRow?>(null) }
    var sharedSelectedChild by remember { mutableStateOf<String?>(null) }
    var recipientFamilies by remember { mutableStateOf<List<FamilyRef>>(emptyList()) }
    var selectedRecipient by remember { mutableStateOf<String?>(null) }
    var sharedChildName by remember { mutableStateOf("") }
    var inviteAsManager by remember { mutableStateOf(false) }
    val errorText = stringResource(R.string.error)
    val savedText = stringResource(R.string.saved)
    val restoredText = stringResource(R.string.restored)
    LaunchedEffect(selectedFamily) {
        val family = families.find { it.familyId.key() == selectedFamily }
        relayOrigin = family?.let(lastRelayOrigin).orEmpty()
    }
    var pendingBackup by remember { mutableStateOf<BackupFileRow?>(null) }
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
            val (delayed, blocked, recipientStages) = withContext(Dispatchers.IO) {
                var failed = false
                var blocked = false
                val stages = mutableMapOf<String, uniffi.babytrack_core_ffi.RecipientSyncRow>()
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
                        .onFailure { failed = true; if (it is SharedUploadBlocked) blocked = true }
                }
                Triple(failed, blocked, stages)
            }
            automaticSyncDelayed = delayed
            automaticSyncBlocked = blocked
            recipientStages[selectedRecipient]?.let { progress ->
                joinStage = when {
                    progress.ready -> context.getString(R.string.history_ready_auto)
                    progress.awaitingGrant -> context.getString(R.string.history_awaiting_grant, progress.pendingControlCursor.toLong())
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
    fun change(action: () -> Unit) {
        scope.launch {
            runCatching { withContext(Dispatchers.IO) { action() } }
                .onSuccess { version++; message = null }
                .onFailure { message = errorText }
        }
    }
    LaunchedEffect(version, selectedFamily, selectedChild, selectedRecipient) {
        runCatching {
            withContext(Dispatchers.IO) {
                val all = store.families()
                val recipients = sharing.recipientFamilies()
                val recipient = recipients.find { it.familyId.key() == selectedRecipient } ?: recipients.firstOrNull()
                val joinedSnapshot = recipient?.let { runCatching { sharing.snapshot(it) }.getOrNull() }
                val family = all.find { it.familyId.key() == selectedFamily } ?: all.firstOrNull()
                val shared = family?.let(sharing::isShared) ?: false
                val snapshot = if (shared) sharing.snapshot(family) else null
                val kids = snapshot?.children ?: family?.let(store::children).orEmpty()
                val child = kids.find { it.id.key() == selectedChild } ?: kids.firstOrNull()
                val history = if (family != null && child != null) {
                    snapshot?.activities?.filter { it.childId.contentEquals(child.id) }
                        ?: store.timeline(family, child.id)
                } else emptyList()
                ScreenData(
                    all, family?.familyId?.key(), kids, history,
                    if (!shared) family?.let(store::revision) ?: 0uL else 0uL,
                    if (!shared) family?.let(store::restoredOrigin) else null,
                    shared,
                    snapshot,
                    recipients,
                    joinedSnapshot,
                )
            }
        }.onSuccess { data ->
            val all = data.families
            val kids = data.children
            families = all
            selectedFamily = all.find { it.familyId.key() == selectedFamily }?.familyId?.key() ?: all.firstOrNull()?.familyId?.key()
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
            if (activeShared) activeSharedSnapshot?.let { SharedHealth(it) }
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
                    label = { Text(stringResource(R.string.family_number, index + 1)) },
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
                Card(modifier = Modifier.fillMaxWidth()) {
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
                        Button(enabled = receivedFragment.isNotBlank(), onClick = {
                            joinStage = context.getString(R.string.join_preparing)
                            scope.launch {
                                runCatching {
                                    withContext(Dispatchers.IO) { sharing.claim(receivedFragment.trim()) }
                                }.onSuccess { prepared ->
                                    selectedRecipient = prepared.family.familyId.key()
                                    version++
                                    joinStage = context.getString(R.string.join_pending)
                                    message = null
                                }.onFailure {
                                    joinStage = context.getString(R.string.join_retry)
                                    message = errorText
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
                                    joinStage = if (progress.ready) {
                                        context.getString(R.string.history_ready, progress.childCount.toLong())
                                    } else if (progress.awaitingGrant) {
                                        context.getString(R.string.history_awaiting_grant, progress.pendingControlCursor.toLong())
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
                            SharedHealth(snapshot)
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

            if (family != null) {
                if (BuildConfig.DEBUG) {
                    Card(modifier = Modifier.fillMaxWidth()) {
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
                        scope.launch {
                            runCatching { withContext(Dispatchers.IO) {
                                if (activeShared) sharing.addChild(family, name, System.currentTimeMillis())
                                else store.addChild(family, name, System.currentTimeMillis())
                            } }
                                .onSuccess { created ->
                                    selectedChild = created.key()
                                    childName = ""
                                    version++
                                    message = null
                                }.onFailure { message = errorText }
                        }
                    }) { Text(stringResource(R.string.add_child)) }
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
                    Text(stringResource(R.string.timeline), style = MaterialTheme.typography.titleLarge)
                    if (entries.isEmpty()) Text(stringResource(R.string.no_entries))
                    entries.forEach { entry ->
                        val label = when {
                            entry.bottleMl != null -> stringResource(R.string.bottle, entry.bottleMl!!)
                            entry.diaperKind != null -> stringResource(R.string.diaper, when (entry.diaperKind!!.toInt()) {
                                1 -> stringResource(R.string.wet)
                                2 -> stringResource(R.string.dirty)
                                else -> stringResource(R.string.both)
                            })
                            else -> entry.kind
                        }
                        Card(Modifier.fillMaxWidth()) {
                            Column(Modifier.padding(12.dp)) {
                                Text(label, fontWeight = FontWeight.SemiBold)
                                Text(DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(entry.startUtcMs)))
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
}

private fun nowTime(): ActivityWhen {
    val now = System.currentTimeMillis()
    return ActivityWhen(now, (TimeZone.getDefault().getOffset(now) / 60_000).toShort(), now)
}

@Composable
private fun SharedHealth(snapshot: SharedSnapshotRow) {
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

private fun savedTime(utcMs: Long): String =
    DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(utcMs))
