package org.babytrack.app

import android.content.Context
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.babytrack_core_ffi.FamilyRef
import uniffi.babytrack_core_ffi.NativeLocalStore
import uniffi.babytrack_core_ffi.RecipientSyncRow
import uniffi.babytrack_core_ffi.RemovedDeviceRow
import uniffi.babytrack_core_ffi.SharedSnapshotRow
import uniffi.babytrack_core_ffi.SharedSyncRow

internal class TrackerSharingState {
    var automaticSyncDelayed by mutableStateOf(false)
    var automaticSyncBlocked by mutableStateOf(false)
    var relayOrigin by mutableStateOf("")
    var relayPublicKey by mutableStateOf("")
    var shareStage by mutableStateOf<String?>(null)
    var shareInProgress by mutableStateOf(false)
    var inviteInProgress by mutableStateOf(false)
    var invitationFragment by mutableStateOf<String?>(null)
    var receivedFragment by mutableStateOf("")
    var showJoinForm by mutableStateOf(false)
    var showShareForm by mutableStateOf(false)
    var showFamilySetup by mutableStateOf(false)
    var showAccessControls by mutableStateOf(false)
    var showDataControls by mutableStateOf(false)
    var joinStage by mutableStateOf<String?>(null)
    var joinInProgress by mutableStateOf(false)
    var selectedRecipient by mutableStateOf<String?>(null)
    var inviteAsManager by mutableStateOf(false)
}

internal data class TrackerSyncPass(
    val delayed: Boolean,
    val blocked: Boolean,
    val recipients: Map<String, RecipientSyncRow>,
    val terminals: Map<String, InvitationTerminalReason>,
    val removals: Map<String, RemovedDeviceRow>,
)

internal fun advanceTrackerSharing(
    store: NativeLocalStore,
    sharing: ShareCoordinator,
    lastRelayOrigin: (FamilyRef) -> String?,
): TrackerSyncPass {
    var failed = false
    var blocked = false
    val stages = mutableMapOf<String, uniffi.babytrack_core_ffi.RecipientSyncRow>()
    val terminals = mutableMapOf<String, InvitationTerminalReason>()
    val removals = mutableMapOf<String, RemovedDeviceRow>()
    val localFamilies =
        runCatching { store.families() }
            .getOrElse {
                if (it is kotlinx.coroutines.CancellationException) throw it
                Log.w("BabytrackSync", "Could not list local Families", it)
                failed = true
                emptyList()
            }
    for (family in localFamilies) {
        val origin = lastRelayOrigin(family) ?: continue
        val canAdvance =
            runCatching { sharing.isShared(family) && !sharing.isRemoved(family) }
                .getOrElse {
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
                    else {
                        failed = true
                        if (it is SharedUploadBlocked) blocked = true
                    }
                }
        }
    }
    val recipients =
        runCatching { sharing.recipientFamilies() }
            .getOrElse {
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
                else {
                    failed = true
                    if (it is SharedUploadBlocked) blocked = true
                }
            }
    }
    return TrackerSyncPass(failed, blocked, stages, terminals, removals)
}

internal data class TrackerSharingActions(
    val onJoinOrRetry: () -> Unit,
    val onSyncShared: (SharedSnapshotRow) -> Unit,
    val onCreateInvite: (SharedSnapshotRow) -> Unit,
    val onShareFamilyAction: () -> Unit,
    val onShareRetryButton: () -> Unit,
    val onSyncManager: () -> Unit,
)

internal fun trackerSharingActions(
    context: Context,
    state: TrackerSharingState,
    feedback: TrackerFeedback,
    scope: CoroutineScope,
    sharing: ShareCoordinator,
    family: FamilyRef?,
    recipientFamilies: List<FamilyRef>,
    inviteOrigin: String,
    selectedFamily: () -> String?,
    onInvitationConsumed: () -> Unit,
    recordRelayOrigin: (FamilyRef, String) -> Boolean,
    errorText: String,
): TrackerSharingActions =
    with(state) {
        with(feedback) {
            TrackerSharingActions(
                onJoinOrRetry = action@{
                        joinInProgress = true
                        joinStage = context.getString(R.string.join_preparing)
                        scope.launch {
                            runCatching {
                                    withContext(Dispatchers.IO) {
                                        val recipient =
                                            recipientFamilies.find {
                                                it.familyId.key() == selectedRecipient
                                            }
                                        val prepared =
                                            if (receivedFragment.isNotBlank())
                                                sharing.claim(receivedFragment.trim())
                                            else
                                                sharing.retryClaim(
                                                    recipient ?: error("No saved recipient claim")
                                                )
                                        prepared to
                                            runCatching {
                                                sharing.advanceRecipient(prepared.family)
                                            }
                                    }
                                }
                                .onSuccess { (prepared, result) ->
                                    selectedRecipient = prepared.family.familyId.key()
                                    receivedFragment = ""
                                    onInvitationConsumed()
                                    version++
                                    val progress = result.getOrNull()
                                    joinStage =
                                        when {
                                            progress?.ready == true ->
                                                context.getString(R.string.history_ready_auto)
                                            progress?.joinPhase == 8u.toUByte() ->
                                                context.getString(R.string.pending_join_removed)
                                            progress?.awaitingGrant == true ->
                                                pendingRecipientMessage(context, progress)
                                            progress != null ->
                                                context.getString(
                                                    R.string.history_pending,
                                                    progress.verifiedCursor.toLong(),
                                                )
                                            result.exceptionOrNull() is InvitationTerminal ->
                                                terminalInvitationMessage(
                                                    context,
                                                    (result.exceptionOrNull() as InvitationTerminal)
                                                        .reason,
                                                )
                                            else ->
                                                context.getString(R.string.join_progress_delayed)
                                        }
                                    message = null
                                }
                                .onFailure { failure ->
                                    joinStage =
                                        (failure as? InvitationTerminal)?.reason?.let {
                                            terminalInvitationMessage(context, it)
                                        } ?: context.getString(R.string.join_retry)
                                    message = if (failure is InvitationTerminal) null else errorText
                                    version++
                                }
                            joinInProgress = false
                        }
                    },
                onSyncShared = action@{ snapshot ->
                        scope.launch {
                            runCatching {
                                    withContext(Dispatchers.IO) {
                                        sharing.syncRecipientAndUpload(snapshot.family)
                                    }
                                }
                                .onSuccess { progress ->
                                    version++
                                    message = sharedSyncMessage(context, progress)
                                }
                                .onFailure {
                                    message =
                                        if (it is SharedUploadBlocked)
                                            context.getString(R.string.shared_upload_blocked)
                                        else errorText
                                }
                        }
                    },
                onCreateInvite = action@{ snapshot ->
                        val invitedFamily = snapshot.family
                        val inviteRole = if (inviteAsManager) 2u.toUByte() else 1u.toUByte()
                        inviteInProgress = true
                        shareStage = context.getString(R.string.invite_preparing)
                        scope.launch {
                            runCatching {
                                    withContext(Dispatchers.IO) {
                                        sharing.invite(invitedFamily, inviteOrigin, inviteRole)
                                    }
                                }
                                .onSuccess { fragment ->
                                    version++
                                    if (selectedFamily() == invitedFamily.familyId.key()) {
                                        invitationFragment = fragment
                                        shareStage = context.getString(R.string.invite_confirmed)
                                        message = null
                                    }
                                }
                                .onFailure {
                                    if (it is kotlinx.coroutines.CancellationException) throw it
                                    if (selectedFamily() == invitedFamily.familyId.key()) {
                                        shareStage = context.getString(R.string.share_retry)
                                        message = errorText
                                    }
                                }
                            inviteInProgress = false
                        }
                    },
                onShareFamilyAction = action@{
                        val family = family ?: return@action
                        shareInProgress = true
                        shareStage = context.getString(R.string.share_preparing)
                        scope.launch {
                            runCatching {
                                    withContext(Dispatchers.IO) {
                                        val cursor =
                                            sharing.promote(
                                                family,
                                                PreviewRelay.origin,
                                                PreviewRelay.publicKey,
                                            )
                                        cursor to recordRelayOrigin(family, PreviewRelay.origin)
                                    }
                                }
                                .onSuccess { (cursor, savedOrigin) ->
                                    version++
                                    if (selectedFamily() == family.familyId.key()) {
                                        relayOrigin = PreviewRelay.origin
                                        shareStage =
                                            if (savedOrigin)
                                                context.getString(
                                                    R.string.share_confirmed,
                                                    cursor.toLong(),
                                                )
                                            else context.getString(R.string.share_origin_not_saved)
                                        showAccessControls = true
                                        message = if (savedOrigin) null else errorText
                                    }
                                }
                                .onFailure {
                                    if (it is kotlinx.coroutines.CancellationException) throw it
                                    if (selectedFamily() == family.familyId.key()) {
                                        shareStage = context.getString(R.string.share_retry)
                                        message = errorText
                                    }
                                }
                            shareInProgress = false
                        }
                    },
                onShareRetryButton = action@{
                        val family = family ?: return@action
                        shareStage = context.getString(R.string.share_preparing)
                        scope.launch {
                            runCatching {
                                    withContext(Dispatchers.IO) {
                                        sharing.promote(family, relayOrigin.trim(), relayPublicKey)
                                    }
                                }
                                .onSuccess { cursor ->
                                    shareStage =
                                        context.getString(R.string.share_confirmed, cursor.toLong())
                                    version++
                                    message =
                                        if (recordRelayOrigin(family, relayOrigin.trim())) null
                                        else errorText
                                }
                                .onFailure {
                                    shareStage = context.getString(R.string.share_retry)
                                    message = errorText
                                }
                        }
                    },
                onSyncManager = action@{
                        val family = family ?: return@action
                        scope.launch {
                            runCatching {
                                    withContext(Dispatchers.IO) {
                                        sharing.syncAndUpload(family, relayOrigin.trim())
                                    }
                                }
                                .onSuccess { progress ->
                                    shareStage = sharedSyncMessage(context, progress)
                                    version++
                                    message = null
                                }
                                .onFailure {
                                    message =
                                        if (it is SharedUploadBlocked)
                                            context.getString(R.string.shared_upload_blocked)
                                        else errorText
                                }
                        }
                    },
            )
        }
    }

internal fun terminalInvitationMessage(context: Context, reason: InvitationTerminalReason): String =
    context.getString(
        when (reason) {
            InvitationTerminalReason.CLAIMED -> R.string.join_claimed
            InvitationTerminalReason.CANCELED -> R.string.join_canceled
            InvitationTerminalReason.EXPIRED -> R.string.join_expired
            InvitationTerminalReason.ISSUER_INVALID -> R.string.join_issuer_invalid
        }
    )

internal fun sharedSyncMessage(context: Context, progress: SharedSyncRow): String =
    when {
        !progress.ready ->
            context.getString(R.string.history_pending, progress.verifiedCursor.toLong())
        progress.outboxState == 2.toUByte() -> context.getString(R.string.shared_upload_uncertain)
        progress.outboxState == 1.toUByte() -> context.getString(R.string.shared_upload_pending)
        else -> context.getString(R.string.shared_synced, progress.verifiedCursor.toLong())
    }

internal fun pendingRecipientMessage(
    context: Context,
    progress: uniffi.babytrack_core_ffi.RecipientSyncRow,
): String =
    when (progress.joinPhase.toInt()) {
        2 -> context.getString(R.string.history_awaiting_challenge)
        3 -> context.getString(R.string.history_challenge_received)
        4 -> context.getString(R.string.history_proof_committed)
        else ->
            context.getString(
                R.string.history_awaiting_grant,
                progress.pendingControlCursor.toLong(),
            )
    }

internal fun removedHistoryMessage(
    context: Context,
    progress: uniffi.babytrack_core_ffi.RecipientSyncRow,
): String =
    when {
        progress.privateCopy == null -> context.getString(R.string.history_removed)
        progress.pendingResult == 0.toUByte() -> context.getString(R.string.history_removed_unsent)
        progress.pendingResult == 2.toUByte() ->
            context.getString(R.string.history_removed_accepted)
        progress.pendingResult == 3.toUByte() ->
            context.getString(R.string.history_removed_rejected)
        else -> context.getString(R.string.history_removed_copied)
    }
