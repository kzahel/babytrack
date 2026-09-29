package org.babytrack.app

import android.app.ActivityManager
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

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
            ActivityManager.MemoryInfo()
                .also {
                    (getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager).getMemoryInfo(
                        it
                    )
                }
                .availMem
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
                    readFile = { uri ->
                        contentResolver.openInputStream(uri)?.use {
                            readBounded(it, backupReadLimit(availableMemory()))
                        }
                    },
                    writeFile = { uri, bytes ->
                        contentResolver.openOutputStream(uri)?.use { it.write(bytes) }
                            ?: error("No output stream")
                    },
                    availableMemory = availableMemory,
                    lastSave = { family ->
                        savedFiles.getString(family.familyId.key(), null)?.split(':')?.let { parts
                            ->
                            if (parts.size == 2) {
                                val at = parts[0].toLongOrNull()
                                val revision = parts[1].toULongOrNull()
                                if (at != null && revision != null) CompletedSave(at, revision)
                                else null
                            } else null
                        }
                    },
                    recordSave = { file ->
                        savedFiles
                            .edit()
                            .putString(
                                file.info.sourceFamilyId.key(),
                                "${file.info.snapshotUtcMs}:${file.revision}",
                            )
                            .commit()
                    },
                    lastRelayOrigin = { family ->
                        relayOrigins.getString(family.familyId.key(), null)
                    },
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
    val text =
        when (intent?.action) {
            Intent.ACTION_SEND ->
                if (intent.type == "text/plain") {
                    intent.getStringExtra(Intent.EXTRA_TEXT)?.trim()
                } else null
            Intent.ACTION_VIEW -> intent.data?.toString()
            else -> null
        } ?: return null
    val fragment =
        if (text.startsWith("#")) text
        else {
            val uri = runCatching { android.net.Uri.parse(text) }.getOrNull() ?: return null
            if (
                uri.scheme != "babytrack" ||
                    uri.host != "join" ||
                    !uri.path.isNullOrEmpty() ||
                    uri.port != -1 ||
                    uri.userInfo != null ||
                    uri.query != null
            )
                return null
            "#${uri.encodedFragment ?: return null}"
        }
    return fragment.takeIf { it.length <= 2048 && it.startsWith("#bt-invite=v1.") }
}

internal fun invitationLink(fragment: String): String = "babytrack://join$fragment"

internal fun browserInvitationLink(origin: String, fragment: String): String = "$origin/$fragment"

internal fun shareInvitation(context: Context, link: String) {
    val send =
        Intent(Intent.ACTION_SEND).apply {
            type = "text/plain"
            putExtra(Intent.EXTRA_TEXT, link)
        }
    context.startActivity(Intent.createChooser(send, context.getString(R.string.share_invitation)))
}

internal fun copyInvitation(context: Context, link: String) {
    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    clipboard.setPrimaryClip(
        ClipData.newPlainText(context.getString(R.string.share_invitation), link)
    )
}
