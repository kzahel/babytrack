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
                TrackerRoute(
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

internal fun invitationLink(fragment: String): String = "babytrack://join$fragment"

internal fun browserInvitationLink(origin: String, fragment: String): String = "$origin/$fragment"

internal fun shareInvitation(context: Context, link: String) {
    val send = Intent(Intent.ACTION_SEND).apply {
        type = "text/plain"
        putExtra(Intent.EXTRA_TEXT, link)
    }
    context.startActivity(Intent.createChooser(send, context.getString(R.string.share_invitation)))
}

internal fun copyInvitation(context: Context, link: String) {
    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    clipboard.setPrimaryClip(ClipData.newPlainText(context.getString(R.string.share_invitation), link))
}

