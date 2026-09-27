package org.babytrack.app

import android.os.Bundle
import android.app.ActivityManager
import android.content.Context
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
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
import uniffi.babytrack_core_ffi.ActivityRow
import uniffi.babytrack_core_ffi.ActivityWhen
import uniffi.babytrack_core_ffi.BackupFileRow
import uniffi.babytrack_core_ffi.BackupInfoRow
import uniffi.babytrack_core_ffi.ChildRow
import uniffi.babytrack_core_ffi.FamilyRef
import uniffi.babytrack_core_ffi.NativeLocalStore
import uniffi.babytrack_core_ffi.RestoredOriginRow
import java.text.DateFormat
import java.util.Date
import java.util.TimeZone

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val database = filesDir.resolve("families.db")
        val savedFiles = getSharedPreferences("completed_file_saves", MODE_PRIVATE)
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
                )
            }
        }
    }
}

private fun ByteArray.key(): String = joinToString("") { "%02x".format(it) }

private data class CompletedSave(val atMs: Long, val revision: ULong)
private data class ScreenData(
    val families: List<FamilyRef>,
    val children: List<ChildRow>,
    val entries: List<ActivityRow>,
    val revision: ULong,
    val restoredOrigin: RestoredOriginRow?,
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
) {
    val context = LocalContext.current
    DisposableEffect(store, sharing) { onDispose { store.close(); sharing.close() } }
    val scope = rememberCoroutineScope()
    var version by remember { mutableStateOf(0) }
    var families by remember { mutableStateOf<List<FamilyRef>>(emptyList()) }
    var children by remember { mutableStateOf<List<ChildRow>>(emptyList()) }
    var entries by remember { mutableStateOf<List<ActivityRow>>(emptyList()) }
    var revision by remember { mutableStateOf(0uL) }
    var restoredOrigin by remember { mutableStateOf<RestoredOriginRow?>(null) }
    var saveStatusVersion by remember { mutableStateOf(0) }
    var selectedFamily by remember { mutableStateOf<String?>(null) }
    var selectedChild by remember { mutableStateOf<String?>(null) }
    var childName by remember { mutableStateOf("") }
    var amount by remember { mutableStateOf("") }
    var message by remember { mutableStateOf<String?>(null) }
    var relayOrigin by remember { mutableStateOf("") }
    var relayPublicKey by remember { mutableStateOf("") }
    var shareStage by remember { mutableStateOf<String?>(null) }
    var invitationFragment by remember { mutableStateOf<String?>(null) }
    var receivedFragment by remember { mutableStateOf("") }
    var joinStage by remember { mutableStateOf<String?>(null) }
    var inviteAsManager by remember { mutableStateOf(false) }
    val errorText = stringResource(R.string.error)
    val savedText = stringResource(R.string.saved)
    val restoredText = stringResource(R.string.restored)
    var pendingBackup by remember { mutableStateOf<BackupFileRow?>(null) }
    var protectBackup by remember { mutableStateOf(false) }
    var backupPassword by remember { mutableStateOf("") }
    var restorePassword by remember { mutableStateOf("") }
    var pendingRestore by remember { mutableStateOf<ByteArray?>(null) }
    var pendingRestoreProtected by remember { mutableStateOf(false) }
    var restoreInfo by remember { mutableStateOf<BackupInfoRow?>(null) }
    val passwordNeeded = stringResource(R.string.password_needed)
    val passwordOrFileError = stringResource(R.string.password_or_file_error)
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
    LaunchedEffect(version, selectedFamily, selectedChild) {
        runCatching {
            withContext(Dispatchers.IO) {
                val all = store.families()
                val family = all.find { it.familyId.key() == selectedFamily } ?: all.firstOrNull()
                val kids = family?.let(store::children).orEmpty()
                val child = kids.find { it.id.key() == selectedChild } ?: kids.firstOrNull()
                val history = if (family != null && child != null) store.timeline(family, child.id) else emptyList()
                ScreenData(
                    all, kids, history,
                    family?.let(store::revision) ?: 0uL,
                    family?.let(store::restoredOrigin),
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
        }.onFailure { message = errorText }
    }
    val family = families.find { it.familyId.key() == selectedFamily }
    val child = children.find { it.id.key() == selectedChild }
    val completed = remember(selectedFamily, saveStatusVersion) { family?.let(lastSave) }
    val filename = stringResource(R.string.backup_filename)
    val protectedFilename = stringResource(R.string.protected_backup_filename)

    Scaffold(topBar = { TopAppBar(title = { Text(stringResource(R.string.screen_title)) }) }) { padding ->
        Column(
            modifier = Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(stringResource(R.string.local_only), style = MaterialTheme.typography.labelMedium)
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
                                }.onSuccess {
                                    joinStage = context.getString(R.string.join_pending)
                                    message = null
                                }.onFailure {
                                    joinStage = context.getString(R.string.join_retry)
                                    message = errorText
                                }
                            }
                        }) { Text(stringResource(R.string.join_or_retry)) }
                        OutlinedButton(enabled = receivedFragment.isNotBlank(), onClick = {
                            joinStage = context.getString(R.string.proof_preparing)
                            scope.launch {
                                runCatching {
                                    withContext(Dispatchers.IO) { sharing.proveChallenge(receivedFragment.trim()) }
                                }.onSuccess {
                                    joinStage = context.getString(R.string.proof_confirmed)
                                    message = null
                                }.onFailure {
                                    joinStage = context.getString(R.string.join_retry)
                                    message = errorText
                                }
                            }
                        }) { Text(stringResource(R.string.prove_challenge)) }
                        joinStage?.let { Text(it) }
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
                                        message = null
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
                            runCatching { withContext(Dispatchers.IO) { store.addChild(family, name, System.currentTimeMillis()) } }
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
                            Button(onClick = { change { store.logDiaper(family, child.id, kind, nowTime()) } }) {
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
                            change { store.logBottleMl(family, child.id, ml, 2u.toUByte(), nowTime()) }
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
                completed?.let { saved ->
                    Text(stringResource(R.string.last_saved_at, savedTime(saved.atMs)))
                    if (revision > saved.revision) Text(stringResource(R.string.changes_after_save))
                }
                Text(stringResource(if (protectBackup) R.string.protected_backup_description else R.string.backup_description))
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
                            store.backupFile(
                                family,
                                System.currentTimeMillis(),
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

private fun savedTime(utcMs: Long): String =
    DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(utcMs))
