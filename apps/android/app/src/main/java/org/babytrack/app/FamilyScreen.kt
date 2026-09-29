package org.babytrack.app

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import uniffi.babytrack_core_ffi.BackupInfoRow
import uniffi.babytrack_core_ffi.ChildRow
import uniffi.babytrack_core_ffi.FamilyRef
import uniffi.babytrack_core_ffi.PendingDeviceRow
import uniffi.babytrack_core_ffi.RestoredOriginRow
import uniffi.babytrack_core_ffi.SharedDeviceRow
import uniffi.babytrack_core_ffi.SharedSnapshotRow

internal data class FamilyUiState(
    val family: FamilyRef?,
    val child: ChildRow?,
    val families: List<FamilyRef>,
    val children: List<ChildRow>,
    val selectedFamily: String?,
    val selectedChild: String?,
    val familyChildNames: Map<String, String>,
    val removedFamilies: List<FamilyRef>,
    val activeShared: Boolean,
    val activeFamilyIsLocal: Boolean,
    val automaticSyncDelayed: Boolean,
    val automaticSyncBlocked: Boolean,
    val shareStage: String?,
    val activeSharedSnapshot: SharedSnapshotRow?,
    val activeUnusedInvitationIds: List<ByteArray>?,
    val deviceLabels: Map<String, String>,
    val showAccessControls: Boolean,
    val showFamilySetup: Boolean,
    val showShareForm: Boolean,
    val shareInProgress: Boolean,
    val relayOrigin: String,
    val relayPublicKey: String,
    val inviteOrigin: String,
    val inviteAsManager: Boolean,
    val inviteInProgress: Boolean,
    val invitationFragment: String?,
    val joinFirst: Boolean,
    val showJoinForm: Boolean,
    val recipientFamilies: List<FamilyRef>,
    val readyRecipientKeys: Set<String>,
    val selectedRecipient: String?,
    val receivedFragment: String,
    val joinInProgress: Boolean,
    val sharedSnapshot: SharedSnapshotRow?,
    val joinStage: String?,
    val restoredOrigin: RestoredOriginRow?,
    val ageLabel: String,
    val showChildDetails: Boolean,
    val showDataControls: Boolean,
    val completed: CompletedSave?,
    val revision: ULong,
    val protectBackup: Boolean,
    val backupPassword: String,
    val hasPendingRestore: Boolean,
    val pendingRestoreProtected: Boolean,
    val restorePassword: String,
    val restoreInfo: BackupInfoRow?,
)

internal data class FamilyActions(
    val onOpenJoin: () -> Unit = {},
    val onSelectRecipient: (FamilyRef) -> Unit = { _ -> },
    val onReceivedFragmentChange: (String) -> Unit = { _ -> },
    val onJoinOrRetry: () -> Unit = {},
    val onToggleAccess: () -> Unit = {},
    val onNameDevice: (ByteArray, SharedSnapshotRow) -> Unit = { _, _ -> },
    val onSyncShared: (SharedSnapshotRow) -> Unit = { _ -> },
    val onRemoveDevice: (SharedDeviceRow) -> Unit = { _ -> },
    val onPromoteDevice: (SharedDeviceRow, UByte) -> Unit = { _, _ -> },
    val onRemovePendingDevice: (PendingDeviceRow) -> Unit = { _ -> },
    val onCancelInvitation: (ByteArray) -> Unit = { _ -> },
    val onInviteMember: () -> Unit = {},
    val onInviteManager: () -> Unit = {},
    val onCreateInvite: (SharedSnapshotRow) -> Unit = { _ -> },
    val onShareAndroidInvitation: (String) -> Unit = { _ -> },
    val onCopyAndroidInvitation: (String) -> Unit = { _ -> },
    val onShareBrowserInvitation: (String) -> Unit = { _ -> },
    val onMakePrivateCopy: () -> Unit = {},
    val onSelectFamily: (FamilyRef) -> Unit = { _ -> },
    val onContinueInPrivateCopy: (FamilyRef) -> Unit = { _ -> },
    val onToggleFamilyOptions: () -> Unit = {},
    val onNewFamily: () -> Unit = {},
    val onShareFamilyAction: () -> Unit = {},
    val onOpenSharingControls: () -> Unit = {},
    val onRelayOriginChange: (String) -> Unit = { _ -> },
    val onRelayPublicKeyChange: (String) -> Unit = { _ -> },
    val onShareRetryButton: () -> Unit = {},
    val onSyncManager: () -> Unit = {},
    val onSelectChild: (ChildRow) -> Unit = { _ -> },
    val onToggleChildOptions: () -> Unit = {},
    val onEditChildProfile: () -> Unit = {},
    val onAddAnotherChild: () -> Unit = {},
    val onAddChild: () -> Unit = {},
    val onToggleData: () -> Unit = {},
    val onToggleBackupProtection: () -> Unit = {},
    val onBackupPasswordChange: (String) -> Unit = { _ -> },
    val onSaveBackup: () -> Unit = {},
    val onExportAnalysisCsv: () -> Unit = {},
    val onRestoreBackup: () -> Unit = {},
    val onRestorePasswordChange: (String) -> Unit = { _ -> },
    val onInspectProtected: () -> Unit = {},
    val onConfirmRestore: () -> Unit = {},
)

@Composable
internal fun ColumnScope.FamilyScreen(state: FamilyUiState, actions: FamilyActions) {
    val route = TrackerDestination.FAMILY
    with(state) {
        val joinControls: @Composable () -> Unit = {
            if (!showJoinForm && recipientFamilies.isEmpty())
                OutlinedButton(onClick = actions.onOpenJoin) {
                    Text(stringResource(R.string.join_family))
                }
            else
                Card(modifier = Modifier.fillMaxWidth()) {
                    Column(
                        modifier = Modifier.padding(16.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Text(
                            stringResource(R.string.join_family),
                            style = MaterialTheme.typography.titleMedium,
                        )
                        Text(stringResource(R.string.join_description))
                        recipientFamilies.forEachIndexed { index, recipient ->
                            val ready = recipient.familyId.key() in readyRecipientKeys
                            FilterChip(
                                selected = recipient.familyId.key() == selectedRecipient,
                                onClick = { actions.onSelectRecipient(recipient) },
                                label = {
                                    Text(
                                        stringResource(
                                            if (ready) R.string.ready_family_number
                                            else R.string.joining_family_number,
                                            index + 1,
                                        )
                                    )
                                },
                            )
                        }
                        if (selectedRecipient != null && selectedRecipient !in readyRecipientKeys) {
                            Text(stringResource(R.string.saved_join_pending))
                        }
                        OutlinedTextField(
                            value = receivedFragment,
                            onValueChange = { it -> actions.onReceivedFragmentChange(it) },
                            label = { Text(stringResource(R.string.received_fragment)) },
                            modifier = Modifier.fillMaxWidth(),
                            singleLine = true,
                        )
                        Button(
                            enabled =
                                !joinInProgress &&
                                    (receivedFragment.isNotBlank() ||
                                        selectedRecipient != null &&
                                            sharedSnapshot?.family?.familyId?.key() !=
                                                selectedRecipient),
                            onClick = actions.onJoinOrRetry,
                        ) {
                            Text(stringResource(R.string.join_or_retry))
                        }
                        joinStage?.let { Text(it) }
                    }
                }
        }
        if (route == TrackerDestination.FAMILY) {
            if (joinFirst) joinControls()
            if (family == null && !joinFirst) Text(stringResource(R.string.first_run_intro))
            if (family != null)
                Text(
                    stringResource(
                        if (activeShared) R.string.shared_family else R.string.local_only
                    ),
                    style = MaterialTheme.typography.labelMedium,
                )
            if (automaticSyncDelayed && !automaticSyncBlocked)
                Text(stringResource(R.string.automatic_sync_delayed))
            if (automaticSyncBlocked)
                Text(
                    stringResource(R.string.shared_upload_blocked),
                    color = MaterialTheme.colorScheme.error,
                )
            if (activeShared) {
                shareStage?.let { Text(it) }
                activeSharedSnapshot?.let { snapshot ->
                    Text(
                        pluralStringResource(
                            R.plurals.shared_device_count,
                            snapshot.devices.size,
                            snapshot.devices.size,
                        )
                    )
                    if (snapshot.pendingDevices.isNotEmpty()) {
                        Text(
                            pluralStringResource(
                                R.plurals.shared_pending_device_count,
                                snapshot.pendingDevices.size,
                                snapshot.pendingDevices.size,
                            )
                        )
                    }
                    if (snapshot.unsentCount > 0uL) {
                        Text(
                            stringResource(
                                R.string.shared_pending_changes,
                                snapshot.unsentCount.toLong(),
                            )
                        )
                    }
                    if (snapshot.inertCount > 0uL) {
                        Text(
                            stringResource(
                                R.string.shared_unreadable_batches,
                                snapshot.inertCount.toLong(),
                            ),
                            color = MaterialTheme.colorScheme.error,
                        )
                    }
                }
                OutlinedButton(onClick = actions.onToggleAccess) {
                    Text(
                        stringResource(
                            if (showAccessControls) R.string.hide_family_access
                            else R.string.show_family_access
                        )
                    )
                }
            }
            if (activeShared && showAccessControls)
                activeSharedSnapshot?.let { snapshot ->
                    SharedHealth(
                        snapshot,
                        deviceLabels,
                        onNameDevice = { target -> actions.onNameDevice(target, snapshot) },
                    )
                    if (!activeFamilyIsLocal) {
                        Text(stringResource(R.string.shared_manual_sync))
                        OutlinedButton(onClick = { actions.onSyncShared(snapshot) }) {
                            Text(stringResource(R.string.sync_shared))
                        }
                    }
                    if (
                        family != null &&
                            snapshot.devices.any {
                                it.deviceId.contentEquals(family.deviceId) && it.role == 2.toUByte()
                            }
                    ) {
                        snapshot.devices
                            .filterNot { it.deviceId.contentEquals(family.deviceId) }
                            .forEach { device ->
                                val label =
                                    deviceLabels[
                                            deviceLabelKey(
                                                snapshot.family.familyId,
                                                device.deviceId,
                                            )]
                                        ?.takeIf { it.isNotBlank() }
                                        ?: stringResource(
                                            R.string.device_short_id,
                                            device.deviceId.key().take(8),
                                        )
                                OutlinedButton(onClick = { actions.onRemoveDevice(device) }) {
                                    Text(stringResource(R.string.remove_device, label))
                                }
                                if (BuildConfig.DEBUG) {
                                    val nextRole =
                                        if (device.role == 2.toUByte()) 1u.toUByte()
                                        else 2u.toUByte()
                                    OutlinedButton(
                                        onClick = { actions.onPromoteDevice(device, nextRole) }
                                    ) {
                                        Text(
                                            stringResource(
                                                if (nextRole == 2.toUByte()) R.string.promote_device
                                                else R.string.demote_device,
                                                label,
                                            )
                                        )
                                    }
                                }
                            }
                        if (snapshot.pendingDevices.isNotEmpty()) {
                            Text(
                                stringResource(R.string.pending_devices),
                                style = MaterialTheme.typography.titleMedium,
                            )
                            snapshot.pendingDevices.forEach { pending ->
                                val label =
                                    deviceLabels[
                                            deviceLabelKey(
                                                snapshot.family.familyId,
                                                pending.deviceId,
                                            )]
                                        ?.takeIf { it.isNotBlank() }
                                        ?: stringResource(
                                            R.string.device_short_id,
                                            pending.deviceId.key().take(8),
                                        )
                                OutlinedButton(
                                    onClick = { actions.onRemovePendingDevice(pending) }
                                ) {
                                    Text(stringResource(R.string.remove_pending_device, label))
                                }
                            }
                        }
                        if (BuildConfig.DEBUG) {
                            if (activeUnusedInvitationIds == null) {
                                Text(stringResource(R.string.invitation_list_delayed))
                            } else if (!activeUnusedInvitationIds.isNullOrEmpty()) {
                                Text(
                                    stringResource(R.string.unused_invitations),
                                    style = MaterialTheme.typography.titleMedium,
                                )
                                activeUnusedInvitationIds.orEmpty().forEach { invitationId ->
                                    OutlinedButton(
                                        onClick = { actions.onCancelInvitation(invitationId) }
                                    ) {
                                        Text(
                                            stringResource(
                                                R.string.cancel_invitation,
                                                invitationId.key().take(8),
                                            )
                                        )
                                    }
                                }
                            }
                        }

                        if (inviteOrigin.isNotBlank()) {
                            Text(
                                stringResource(R.string.invite_caregiver),
                                style = MaterialTheme.typography.titleMedium,
                            )
                            Text(stringResource(R.string.invite_description))
                            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                FilterChip(
                                    selected = !inviteAsManager,
                                    onClick = actions.onInviteMember,
                                    label = { Text(stringResource(R.string.invite_member)) },
                                )
                                FilterChip(
                                    selected = inviteAsManager,
                                    onClick = actions.onInviteManager,
                                    label = { Text(stringResource(R.string.invite_manager)) },
                                )
                            }
                            OutlinedButton(
                                enabled = !inviteInProgress,
                                onClick = { actions.onCreateInvite(snapshot) },
                            ) {
                                Text(stringResource(R.string.create_invite))
                            }
                            invitationFragment?.let { fragment ->
                                OutlinedButton(
                                    onClick = { actions.onShareAndroidInvitation(fragment) }
                                ) {
                                    Text(stringResource(R.string.share_android_invitation))
                                }
                                OutlinedButton(
                                    onClick = { actions.onCopyAndroidInvitation(fragment) }
                                ) {
                                    Text(stringResource(R.string.copy_android_invitation))
                                }
                                if (inviteOrigin == PreviewRelay.origin)
                                    OutlinedButton(
                                        onClick = { actions.onShareBrowserInvitation(fragment) }
                                    ) {
                                        Text(stringResource(R.string.share_browser_invitation))
                                    }
                            }
                        }
                    }
                }
            if (activeShared && showAccessControls && family != null)
                OutlinedButton(onClick = actions.onMakePrivateCopy) {
                    Text(stringResource(R.string.make_private_copy))
                }
            if (families.size != 1) {
                Text(stringResource(R.string.families), style = MaterialTheme.typography.titleLarge)
                families.forEachIndexed { index, item ->
                    FilterChip(
                        selected = item.familyId.key() == selectedFamily,
                        onClick = { actions.onSelectFamily(item) },
                        label = {
                            val firstChild = familyChildNames[item.familyId.key()]
                            Text(
                                if (firstChild == null)
                                    stringResource(R.string.family_number, index + 1)
                                else
                                    stringResource(
                                        R.string.family_with_child,
                                        index + 1,
                                        firstChild,
                                    )
                            )
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
                        Text(
                            stringResource(
                                R.string.removed_family_card,
                                source.familyId.key().take(8),
                            )
                        )
                        OutlinedButton(onClick = { actions.onContinueInPrivateCopy(source) }) {
                            Text(stringResource(R.string.continue_in_private_copy))
                        }
                    }
                }
            }
            if (family != null)
                OutlinedButton(onClick = actions.onToggleFamilyOptions) {
                    Text(
                        stringResource(
                            if (showFamilySetup) R.string.hide_family_setup
                            else R.string.show_family_setup
                        )
                    )
                }
            if (family == null || showFamilySetup)
                OutlinedButton(onClick = actions.onNewFamily) {
                    Text(stringResource(R.string.new_family))
                }

            if (!joinFirst && (family == null || showFamilySetup || recipientFamilies.isNotEmpty()))
                joinControls()
        }
        if (route == TrackerDestination.FAMILY && family != null && activeFamilyIsLocal) {
            if (!activeShared && BuildConfig.DEBUG)
                Card(modifier = Modifier.fillMaxWidth()) {
                    Column(
                        modifier = Modifier.padding(16.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Text(
                            stringResource(R.string.share_family),
                            style = MaterialTheme.typography.titleMedium,
                        )
                        Text(stringResource(R.string.preview_share_description))
                        Button(enabled = !shareInProgress, onClick = actions.onShareFamilyAction) {
                            Text(stringResource(R.string.share_family_action))
                        }
                        shareStage?.let { Text(it) }
                    }
                }
            if (showFamilySetup && BuildConfig.DEBUG) {
                if (!showShareForm)
                    OutlinedButton(onClick = actions.onOpenSharingControls) {
                        Text(stringResource(R.string.sharing_controls))
                    }
                else
                    Card(modifier = Modifier.fillMaxWidth()) {
                        Column(
                            modifier = Modifier.padding(16.dp),
                            verticalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            Text(
                                stringResource(R.string.dev_share_title),
                                style = MaterialTheme.typography.titleMedium,
                            )
                            Text(stringResource(R.string.dev_share_description))
                            OutlinedTextField(
                                value = relayOrigin,
                                onValueChange = { it -> actions.onRelayOriginChange(it) },
                                label = { Text(stringResource(R.string.relay_origin)) },
                                modifier = Modifier.fillMaxWidth(),
                                singleLine = true,
                            )
                            OutlinedTextField(
                                value = relayPublicKey,
                                onValueChange = { it -> actions.onRelayPublicKeyChange(it) },
                                label = { Text(stringResource(R.string.relay_public_key)) },
                                modifier = Modifier.fillMaxWidth(),
                                singleLine = true,
                            )
                            Button(onClick = actions.onShareRetryButton) {
                                Text(stringResource(R.string.share_retry_button))
                            }
                            shareStage?.let { Text(it) }
                            OutlinedButton(onClick = actions.onSyncManager) {
                                Text(stringResource(R.string.sync_shared))
                            }
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
                    Text(
                        stringResource(R.string.children),
                        style = MaterialTheme.typography.titleLarge,
                    )
                    if (children.isEmpty()) Text(stringResource(R.string.no_children))
                    children.forEach { item ->
                        FilterChip(
                            selected = item.id.key() == selectedChild,
                            onClick = { actions.onSelectChild(item) },
                            label = { Text(item.name) },
                        )
                    }
                }
            }
            if (child != null && route == TrackerDestination.FAMILY) {
                Text(ageLabel, style = MaterialTheme.typography.bodyMedium)
                OutlinedButton(onClick = actions.onToggleChildOptions) {
                    Text(
                        stringResource(
                            if (showChildDetails) R.string.hide_child_options
                            else R.string.child_options
                        )
                    )
                }
                if (showChildDetails)
                    OutlinedButton(onClick = actions.onEditChildProfile) {
                        Text(stringResource(R.string.edit_child_profile))
                    }
            }
            if (route == TrackerDestination.FAMILY && children.isNotEmpty() && showChildDetails)
                OutlinedButton(onClick = actions.onAddAnotherChild) {
                    Text(stringResource(R.string.add_another_child))
                }
            if (route == TrackerDestination.FAMILY && children.isEmpty())
                Button(onClick = actions.onAddChild) { Text(stringResource(R.string.add_child)) }

            if (route == TrackerDestination.FAMILY) {
                Spacer(Modifier.height(8.dp))
                OutlinedButton(onClick = actions.onToggleData) {
                    Text(
                        stringResource(
                            if (showDataControls) R.string.hide_data_controls
                            else R.string.show_data_controls
                        )
                    )
                }
                if (showDataControls) {
                    Text(
                        stringResource(R.string.backup_title),
                        style = MaterialTheme.typography.titleLarge,
                    )
                    if (!activeShared)
                        completed?.let { saved ->
                            Text(stringResource(R.string.last_saved_at, savedTime(saved.atMs)))
                            if (revision > saved.revision)
                                Text(stringResource(R.string.changes_after_save))
                        }
                    Text(
                        stringResource(
                            if (activeShared) R.string.shared_backup_description
                            else if (protectBackup) R.string.protected_backup_description
                            else R.string.backup_description
                        )
                    )
                    FilterChip(
                        selected = protectBackup,
                        onClick = actions.onToggleBackupProtection,
                        label = { Text(stringResource(R.string.protect_backup)) },
                    )
                    if (protectBackup)
                        OutlinedTextField(
                            value = backupPassword,
                            onValueChange = { it -> actions.onBackupPasswordChange(it) },
                            label = { Text(stringResource(R.string.backup_password)) },
                            visualTransformation = PasswordVisualTransformation(),
                            singleLine = true,
                        )
                    Button(
                        onClick = actions.onSaveBackup,
                        enabled = !protectBackup || backupPassword.isNotEmpty(),
                    ) {
                        Text(stringResource(R.string.save_backup))
                    }
                    OutlinedButton(onClick = actions.onExportAnalysisCsv) {
                        Text(stringResource(R.string.export_analysis_csv))
                    }
                }
            }
        }
        if (route == TrackerDestination.FAMILY && (family == null || showDataControls)) {
            OutlinedButton(onClick = actions.onRestoreBackup) {
                Text(stringResource(R.string.restore_backup))
            }
            if (hasPendingRestore && pendingRestoreProtected) {
                OutlinedTextField(
                    value = restorePassword,
                    onValueChange = { it -> actions.onRestorePasswordChange(it) },
                    label = { Text(stringResource(R.string.restore_password)) },
                    visualTransformation = PasswordVisualTransformation(),
                    singleLine = true,
                )
                Button(
                    enabled = restorePassword.isNotEmpty(),
                    onClick = actions.onInspectProtected,
                ) {
                    Text(stringResource(R.string.inspect_protected))
                }
            }
            restoreInfo?.let { info ->
                Text(
                    stringResource(
                        R.string.file_saved_at,
                        savedTime(info.snapshotUtcMs),
                        info.recordCount.toLong(),
                    )
                )
                if (info.knownGap) Text(stringResource(R.string.file_known_gap))
                Button(onClick = actions.onConfirmRestore) {
                    Text(stringResource(R.string.confirm_restore))
                }
            }
        }
    }
}

@Composable
internal fun SharedHealth(
    snapshot: SharedSnapshotRow,
    deviceLabels: Map<String, String>,
    onNameDevice: (ByteArray) -> Unit,
) {
    Text(stringResource(R.string.shared_devices), style = MaterialTheme.typography.titleMedium)
    snapshot.devices.forEach { device ->
        val role =
            stringResource(
                if (device.role == 2.toUByte()) R.string.invite_manager else R.string.invite_member
            )
        val who =
            if (device.deviceId.contentEquals(snapshot.family.deviceId)) {
                stringResource(R.string.this_device)
            } else {
                stringResource(R.string.another_device)
            }
        val label =
            deviceLabels[deviceLabelKey(snapshot.family.familyId, device.deviceId)]?.takeIf {
                it.isNotBlank()
            }
        Text(
            if (label == null)
                stringResource(R.string.shared_device_row, who, role, device.deviceId.key())
            else
                stringResource(
                    R.string.shared_named_device_row,
                    label,
                    who,
                    role,
                    device.deviceId.key(),
                )
        )
        OutlinedButton(onClick = { onNameDevice(device.deviceId) }) {
            Text(stringResource(R.string.name_device))
        }
    }
}
