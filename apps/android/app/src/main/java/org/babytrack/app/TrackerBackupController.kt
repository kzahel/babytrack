package org.babytrack.app

import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.babytrack_core_ffi.BackupFileRow
import uniffi.babytrack_core_ffi.BackupInfoRow
import uniffi.babytrack_core_ffi.FamilyRef
import uniffi.babytrack_core_ffi.NativeLocalStore

internal class TrackerBackupState {
    var pendingBackup by mutableStateOf<BackupFileRow?>(null)
    var pendingAnalysisCsv by mutableStateOf<ByteArray?>(null)
    var protectBackup by mutableStateOf(false)
    var backupPassword by mutableStateOf("")
    var restorePassword by mutableStateOf("")
    var pendingRestore by mutableStateOf<ByteArray?>(null)
    var pendingRestoreProtected by mutableStateOf(false)
    var restoreInfo by mutableStateOf<BackupInfoRow?>(null)
    var saveStatusVersion by mutableStateOf(0)
}

internal data class TrackerBackupActions(
    val onToggleBackupProtection: () -> Unit,
    val onBackupPasswordChange: (String) -> Unit,
    val onSaveBackup: () -> Unit,
    val onExportAnalysisCsv: () -> Unit,
    val onRestoreBackup: () -> Unit,
    val onRestorePasswordChange: (String) -> Unit,
    val onInspectProtected: () -> Unit,
    val onConfirmRestore: () -> Unit,
)

@Composable
internal fun trackerBackupActions(
    state: TrackerBackupState,
    feedback: TrackerFeedback,
    scope: CoroutineScope,
    store: NativeLocalStore,
    sharing: ShareCoordinator,
    family: FamilyRef?,
    activeShared: Boolean,
    readFile: (Uri) -> ByteArray?,
    writeFile: (Uri, ByteArray) -> Unit,
    availableMemory: () -> Long,
    recordSave: (BackupFileRow) -> Boolean,
    onRestored: (FamilyRef) -> Unit,
    onRestoreCancelled: () -> Unit = {},
): TrackerBackupActions {
    val context = LocalContext.current
    val errorText = stringResource(R.string.error)
    val savedText = stringResource(R.string.saved)
    val restoredText = stringResource(R.string.restored)
    val filename = stringResource(R.string.backup_filename)
    val protectedFilename = stringResource(R.string.protected_backup_filename)
    with(state) {
        with(feedback) {
            val passwordNeeded = stringResource(R.string.password_needed)
            val passwordOrFileError = stringResource(R.string.password_or_file_error)
            val damagedBackupError = stringResource(R.string.damaged_backup_error)
            val tooLargeError = stringResource(R.string.backup_too_large)
            val saveLauncher =
                rememberLauncherForActivityResult(
                    ActivityResultContracts.CreateDocument("application/octet-stream")
                ) { uri ->
                    if (uri != null)
                        scope.launch {
                            runCatching {
                                    withContext(Dispatchers.IO) {
                                        val file = pendingBackup ?: error("Missing backup")
                                        writeFile(uri, file.bytes)
                                        check(recordSave(file))
                                    }
                                }
                                .onSuccess {
                                    message = savedText
                                    saveStatusVersion++
                                }
                                .onFailure { message = errorText }
                            pendingBackup = null
                        }
                }
            val csvLauncher =
                rememberLauncherForActivityResult(
                    ActivityResultContracts.CreateDocument("text/csv")
                ) { uri ->
                    if (uri != null)
                        scope.launch {
                            runCatching {
                                    withContext(Dispatchers.IO) {
                                        writeFile(
                                            uri,
                                            pendingAnalysisCsv ?: error("Missing analysis export"),
                                        )
                                    }
                                }
                                .onSuccess {
                                    message = context.getString(R.string.analysis_csv_saved)
                                }
                                .onFailure { message = errorText }
                            pendingAnalysisCsv = null
                        }
                    else pendingAnalysisCsv = null
                }
            val restoreLauncher =
                rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
                    if (uri == null) onRestoreCancelled()
                    else
                        scope.launch {
                            pendingRestore = null
                            restoreInfo = null
                            restorePassword = ""
                            pendingRestoreProtected = false
                            val bytes =
                                runCatching {
                                        withContext(Dispatchers.IO) {
                                            readFile(uri) ?: error("Missing backup")
                                        }
                                    }
                                    .getOrElse {
                                        message =
                                            if (it is BackupTooLarge) tooLargeError else errorText
                                        return@launch
                                    }
                            val protected =
                                bytes.size >= 5 &&
                                    bytes.copyOfRange(0, 5).contentEquals("BTBK1".toByteArray())
                            if (protected) {
                                pendingRestore = bytes
                                pendingRestoreProtected = true
                                message = passwordNeeded
                            } else {
                                runCatching {
                                        withContext(Dispatchers.IO) { store.inspectReadable(bytes) }
                                    }
                                    .onSuccess { info ->
                                        pendingRestore = bytes
                                        restoreInfo = info
                                        message = null
                                    }
                                    .onFailure { message = damagedBackupError }
                            }
                        }
                }
            return TrackerBackupActions(
                onToggleBackupProtection = action@{ protectBackup = !protectBackup },
                onBackupPasswordChange = action@{ it -> backupPassword = it },
                onSaveBackup = action@{
                        val family = family ?: return@action
                        scope.launch {
                            val password = backupPassword
                            val protected = protectBackup
                            runCatching {
                                    withContext(Dispatchers.IO) {
                                        if (activeShared)
                                            sharing.backupFile(
                                                family,
                                                System.currentTimeMillis(),
                                                if (protected) password else null,
                                                availableMemory().toULong(),
                                            )
                                        else
                                            store.backupFile(
                                                family,
                                                System.currentTimeMillis(),
                                                if (protected) password else null,
                                                availableMemory().toULong(),
                                            )
                                    }
                                }
                                .onSuccess {
                                    pendingBackup = it
                                    backupPassword = ""
                                    saveLauncher.launch(
                                        if (protected) protectedFilename else filename
                                    )
                                }
                                .onFailure { message = errorText }
                        }
                    },
                onExportAnalysisCsv = action@{
                        val family = family ?: return@action
                        scope.launch {
                            runCatching {
                                    withContext(Dispatchers.IO) {
                                        if (activeShared) sharing.analysisCsv(family)
                                        else store.analysisCsv(family)
                                    }
                                }
                                .onSuccess {
                                    pendingAnalysisCsv = it
                                    csvLauncher.launch("babytrack-analysis.csv")
                                }
                                .onFailure { message = errorText }
                        }
                    },
                onRestoreBackup = action@{
                        restoreLauncher.launch(arrayOf("application/octet-stream", "*/*"))
                    },
                onRestorePasswordChange = action@{ it ->
                        restorePassword = it
                        restoreInfo = null
                    },
                onInspectProtected = action@{
                        val bytes = pendingRestore ?: return@action
                        val password = restorePassword
                        scope.launch {
                            runCatching {
                                    withContext(Dispatchers.IO) {
                                        store.inspectProtected(
                                            bytes,
                                            password,
                                            availableMemory().toULong(),
                                        )
                                    }
                                }
                                .onSuccess { info ->
                                    restoreInfo = info
                                    message = null
                                }
                                .onFailure { message = passwordOrFileError }
                        }
                    },
                onConfirmRestore = action@{
                        val bytes = pendingRestore ?: return@action
                        val protected = pendingRestoreProtected
                        val password = restorePassword
                        scope.launch {
                            runCatching {
                                    withContext(Dispatchers.IO) {
                                        if (protected)
                                            store.restoreProtected(
                                                bytes,
                                                password,
                                                availableMemory().toULong(),
                                                System.currentTimeMillis(),
                                            )
                                        else store.restore(bytes, System.currentTimeMillis())
                                    }
                                }
                                .onSuccess { restored ->
                                    pendingRestore = null
                                    restoreInfo = null
                                    restorePassword = ""
                                    onRestored(restored)
                                    version++
                                    message = restoredText
                                }
                                .onFailure {
                                    message = if (protected) passwordOrFileError else errorText
                                }
                        }
                    },
            )
        }
    }
}
