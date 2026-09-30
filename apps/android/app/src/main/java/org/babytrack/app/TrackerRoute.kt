package org.babytrack.app

import android.Manifest
import android.app.DatePickerDialog
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import java.text.DateFormat
import java.time.LocalDate
import java.time.ZoneId
import java.util.Date
import java.util.TimeZone
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.babytrack_core_ffi.ActivityWhen
import uniffi.babytrack_core_ffi.BackupFileRow
import uniffi.babytrack_core_ffi.FamilyRef
import uniffi.babytrack_core_ffi.NativeLocalStore

private data class RemovalCopyNotice(val sourceKey: String, val copy: FamilyRef)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun TrackerRoute(
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
    val feedback = remember { TrackerFeedback() }
    val captureDraft = remember { CaptureDraftState() }
    val edits = remember { EntryEditState() }
    val sharingState = remember { TrackerSharingState() }
    val backupState = remember { TrackerBackupState() }
    with(feedback) {
        with(captureDraft) {
            with(edits) {
                with(sharingState) {
                    with(backupState) {
                        val context = LocalContext.current
                        val activity = context as ComponentActivity
                        var foreground by remember {
                            mutableStateOf(
                                activity.lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)
                            )
                        }
                        DisposableEffect(activity) {
                            val observer = LifecycleEventObserver { _, event ->
                                if (event == Lifecycle.Event.ON_START) foreground = true
                                if (event == Lifecycle.Event.ON_STOP) foreground = false
                            }
                            activity.lifecycle.addObserver(observer)
                            onDispose { activity.lifecycle.removeObserver(observer) }
                        }
                        val scope = rememberCoroutineScope()

                        val notificationPermission =
                            rememberLauncherForActivityResult(
                                ActivityResultContracts.RequestPermission()
                            ) {
                                if (it) version++
                            }
                        var screenData by remember { mutableStateOf<ScreenData?>(null) }
                        val families = screenData?.families ?: emptyList()
                        val removedFamilies = screenData?.removedFamilies ?: emptyList()
                        val familyChildNames = screenData?.familyChildNames ?: emptyMap()
                        val children = screenData?.children ?: emptyList()
                        val entries = screenData?.entries ?: emptyList()
                        val daySummary = screenData?.daySummary
                        val daySummaryDay = screenData?.daySummaryDay
                        val revision = screenData?.revision ?: 0uL
                        val restoredOrigin = screenData?.restoredOrigin
                        val isShared = screenData?.shared ?: false
                        val activeSharedSnapshot = screenData?.mainSharedSnapshot
                        val activeUnusedInvitationIds = screenData?.unusedInvitationIds
                        val loadedFamilyKey = screenData?.activeFamilyKey
                        val loadedChildKey = screenData?.activeChildKey
                        val activeFamilyIsLocal = screenData?.activeFamilyIsLocal ?: false
                        val selectionPrefs = remember {
                            context.getSharedPreferences("tracker_selection", Context.MODE_PRIVATE)
                        }
                        val removalNoticePrefs = remember {
                            context.getSharedPreferences(
                                "acknowledged_removal_copies",
                                Context.MODE_PRIVATE,
                            )
                        }
                        var selectedFamily by remember {
                            mutableStateOf(selectionPrefs.getString("family", null))
                        }
                        var selectedChild by remember {
                            mutableStateOf(selectionPrefs.getString("child", null))
                        }
                        var destination by rememberSaveable {
                            mutableStateOf(TrackerDestination.TODAY)
                        }
                        var captureKind by rememberSaveable { mutableStateOf<CaptureKind?>(null) }
                        var showTargetPicker by remember { mutableStateOf(false) }
                        var pendingRemovalNotice by remember {
                            mutableStateOf<RemovalCopyNotice?>(null)
                        }
                        LaunchedEffect(foreground, version, pendingRemovalNotice) {
                            if (!foreground || pendingRemovalNotice != null) return@LaunchedEffect
                            runCatching {
                                    withContext(Dispatchers.IO) {
                                        (store.families() + sharing.recipientFamilies())
                                            .distinctBy { it.familyId.key() }
                                            .firstNotNullOfOrNull { source ->
                                                val copy =
                                                    sharing.savedRemovalCopy(source)
                                                        ?: return@firstNotNullOfOrNull null
                                                val sourceKey = source.familyId.key()
                                                if (
                                                    removalNoticePrefs.getString(sourceKey, null) ==
                                                        copy.familyId.key()
                                                )
                                                    null
                                                else RemovalCopyNotice(sourceKey, copy)
                                            }
                                    }
                                }
                                .onSuccess { pendingRemovalNotice = it }
                                .onFailure {
                                    Log.w(
                                        "BabytrackRemoval",
                                        "Could not read saved copy destination",
                                        it,
                                    )
                                }
                        }
                        LaunchedEffect(loadedFamilyKey, selectedFamily, selectedChild, children) {
                            if (selectedFamily != null && loadedFamilyKey == selectedFamily) {
                                val child =
                                    selectedChild.takeIf { chosen ->
                                        children.any { it.id.key() == chosen }
                                    }
                                selectionPrefs
                                    .edit()
                                    .putString("family", selectedFamily)
                                    .putString("child", child)
                                    .apply()
                            }
                        }
                        var childName by remember { mutableStateOf("") }
                        var showAddChildForm by remember { mutableStateOf(false) }
                        var showChildDetails by remember { mutableStateOf(false) }

                        var childBirthDate by remember { mutableStateOf("") }
                        var childSex by remember { mutableStateOf(3u.toUByte()) }

                        var timelineFilter by remember { mutableStateOf(TimelineFilter.ALL) }
                        var selectedHistoryDay by rememberSaveable { mutableStateOf<String?>(null) }
                        var expandedEntryKey by remember { mutableStateOf<String?>(null) }

                        val snackbarHostState = remember { SnackbarHostState() }
                        LaunchedEffect(message) {
                            message?.let { snackbarHostState.showSnackbar(it) }
                        }
                        var removalTarget by remember { mutableStateOf<ByteArray?>(null) }
                        var cancelInvitationTarget by remember {
                            mutableStateOf<PendingInvitationCancel?>(null)
                        }
                        var pendingDeviceRemoval by remember {
                            mutableStateOf<PendingDeviceRemoval?>(null)
                        }
                        var roleChangeTarget by remember {
                            mutableStateOf<PendingRoleChange?>(null)
                        }
                        val deviceLabelPrefs = remember {
                            context.getSharedPreferences("device_labels", Context.MODE_PRIVATE)
                        }
                        var deviceLabels by remember {
                            mutableStateOf(
                                deviceLabelPrefs.all
                                    .mapNotNull { (key, value) ->
                                        (value as? String)?.let { key to it }
                                    }
                                    .toMap()
                            )
                        }
                        var deviceLabelTarget by remember { mutableStateOf<String?>(null) }
                        var deviceLabelDraft by remember { mutableStateOf("") }

                        LaunchedEffect(recentlyDeleted) {
                            val target = recentlyDeleted ?: return@LaunchedEffect
                            val result =
                                snackbarHostState.showSnackbar(
                                    context.getString(R.string.entry_deleted),
                                    actionLabel = context.getString(R.string.undo),
                                    duration = SnackbarDuration.Long,
                                )
                            if (result == SnackbarResult.ActionPerformed) {
                                runCatching {
                                        withContext(Dispatchers.IO) {
                                            val at = System.currentTimeMillis()
                                            if (target.shared)
                                                sharing.restoreActivity(
                                                    target.family,
                                                    target.childId,
                                                    target.activityId,
                                                    at,
                                                )
                                            else
                                                store.restoreActivity(
                                                    target.family,
                                                    target.childId,
                                                    target.activityId,
                                                    at,
                                                )
                                        }
                                    }
                                    .onSuccess {
                                        version++
                                        message = null
                                    }
                                    .onFailure { message = context.getString(R.string.error) }
                            }
                            recentlyDeleted = null
                        }

                        val sharedSnapshot = screenData?.joinedSnapshot
                        val recipientFamilies = screenData?.recipients ?: emptyList()
                        val readyRecipientKeys = screenData?.readyRecipientKeys ?: emptySet()
                        LaunchedEffect(incomingInvitation) {
                            if (incomingInvitation != null) {
                                receivedFragment = incomingInvitation
                                showJoinForm = true
                                destination = TrackerDestination.FAMILY
                            }
                        }
                        val errorText = stringResource(R.string.error)
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
                            relayOrigin =
                                family
                                    ?.let {
                                        if (
                                            recipientFamilies.any { recipient ->
                                                recipient.familyId.key() == it.familyId.key()
                                            }
                                        ) {
                                            runCatching { sharing.recipientOrigin(it) }.getOrNull()
                                        } else lastRelayOrigin(it)
                                    }
                                    .orEmpty()
                        }
                        LaunchedEffect(selectedFamily, selectedChild) {
                            showTargetPicker = false
                            captureKind = null
                            if (destination == TrackerDestination.CAPTURE)
                                destination = TrackerDestination.TODAY
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
                        LaunchedEffect(foreground, selectedRecipient) {
                            if (foreground)
                                while (isActive) {
                                    val pass =
                                        withContext(Dispatchers.IO) {
                                            advanceTrackerSharing(store, sharing, lastRelayOrigin)
                                        }
                                    automaticSyncDelayed = pass.delayed
                                    automaticSyncBlocked = pass.blocked
                                    val recipientStages = pass.recipients
                                    pass.terminals[selectedRecipient]?.let { reason ->
                                        joinStage = terminalInvitationMessage(context, reason)
                                        showJoinForm = true
                                        screenData = screenData?.copy(joinedSnapshot = null)
                                    }
                                    pass.removals[selectedFamily]?.let { removed ->
                                        val copy = removed.privateCopy
                                        if (copy != null) {
                                            pendingRemovalNotice =
                                                RemovalCopyNotice(
                                                    selectedFamily ?: return@let,
                                                    copy,
                                                )
                                            message =
                                                when (removed.pendingResult) {
                                                    2.toUByte() ->
                                                        context.getString(
                                                            R.string.history_removed_accepted
                                                        )
                                                    3.toUByte() ->
                                                        context.getString(
                                                            R.string.history_removed_rejected
                                                        )
                                                    0.toUByte() ->
                                                        context.getString(
                                                            R.string.history_removed_unsent
                                                        )
                                                    else ->
                                                        context.getString(
                                                            R.string.history_removed_copied
                                                        )
                                                }
                                        } else message = context.getString(R.string.history_removed)
                                    }
                                    recipientStages[selectedRecipient]?.let { progress ->
                                        if (progress.removed)
                                            screenData = screenData?.copy(joinedSnapshot = null)
                                        joinStage =
                                            when {
                                                progress.removed ->
                                                    removedHistoryMessage(context, progress)
                                                progress.joinPhase == 8u.toUByte() ->
                                                    context.getString(R.string.pending_join_removed)
                                                progress.ready ->
                                                    context.getString(R.string.history_ready_auto)
                                                progress.awaitingGrant ->
                                                    pendingRecipientMessage(context, progress)
                                                else ->
                                                    context.getString(
                                                        R.string.history_pending,
                                                        progress.verifiedCursor.toLong(),
                                                    )
                                            }
                                    }
                                    version++
                                    delay(30_000)
                                }
                        }
                        fun change(onSaved: (() -> Unit)? = null, action: () -> Unit) {
                            scope.launch {
                                runCatching { withContext(Dispatchers.IO) { action() } }
                                    .onSuccess {
                                        version++
                                        message = null
                                        onSaved?.invoke()
                                    }
                                    .onFailure { message = errorText }
                            }
                        }
                        LaunchedEffect(version, selectedFamily, selectedChild, selectedRecipient) {
                            runCatching {
                                    withContext(Dispatchers.IO) {
                                        loadTrackerData(
                                            store,
                                            sharing,
                                            selectedFamily,
                                            selectedChild,
                                            selectedRecipient,
                                        )
                                    }
                                }
                                .onSuccess { data ->
                                    screenData = data
                                    val all = data.families
                                    val kids = data.children
                                    selectedFamily =
                                        all.find { it.familyId.key() == selectedFamily }
                                            ?.familyId
                                            ?.key() ?: all.firstOrNull()?.familyId?.key()
                                    selectedChild =
                                        kids.find { it.id.key() == selectedChild }?.id?.key()
                                            ?: kids.firstOrNull()?.id?.key()
                                    selectedRecipient =
                                        data.recipients
                                            .find { it.familyId.key() == selectedRecipient }
                                            ?.familyId
                                            ?.key()
                                            ?: data.recipients.firstOrNull()?.familyId?.key()
                                    runCatching {
                                            SleepTimerNotifications.update(
                                                context,
                                                data.activeSleepCount,
                                            )
                                        }
                                        .onFailure {
                                            android.util.Log.w(
                                                "BabytrackTimer",
                                                "Could not update sleep notification",
                                                it,
                                            )
                                        }
                                }
                                .onFailure {
                                    if (it is kotlinx.coroutines.CancellationException) throw it
                                    Log.e("BabytrackTracker", "Could not load tracker", it)
                                    message = errorText
                                }
                        }
                        val family = families.find { it.familyId.key() == selectedFamily }
                        val child =
                            children
                                .find { it.id.key() == selectedChild }
                                ?.takeIf { loadedFamilyKey == selectedFamily }
                        val activeShared = isShared && loadedFamilyKey == selectedFamily
                        val completed =
                            remember(selectedFamily, saveStatusVersion) { family?.let(lastSave) }
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
                        val route =
                            if (family == null || child == null) TrackerDestination.FAMILY
                            else destination
                        val scrollState =
                            when (route) {
                                TrackerDestination.TODAY -> todayScrollState
                                TrackerDestination.HISTORY -> historyScrollState
                                TrackerDestination.FAMILY -> familyScrollState
                                TrackerDestination.CAPTURE -> captureScrollState
                            }
                        BackHandler(
                            enabled =
                                route != TrackerDestination.TODAY && family != null && child != null
                        ) {
                            destination = TrackerDestination.TODAY
                            captureKind = null
                        }
                        LaunchedEffect(incomingInvitation) {
                            if (incomingInvitation != null) familyScrollState.scrollTo(0)
                        }
                        val joinFirst =
                            incomingInvitation != null &&
                                (receivedFragment.isNotBlank() || sharedSnapshot == null)

                        fun finishCapture() {
                            if (destination == TrackerDestination.CAPTURE) {
                                destination = TrackerDestination.TODAY
                                captureKind = null
                            }
                        }

                        val inviteOrigin =
                            activeSharedSnapshot
                                ?.let { snapshot ->
                                    if (activeFamilyIsLocal)
                                        lastRelayOrigin(snapshot.family).orEmpty()
                                    else
                                        runCatching { sharing.recipientOrigin(snapshot.family) }
                                            .getOrNull()
                                            .orEmpty()
                                }
                                .orEmpty()

                        val familyNumber =
                            families.indexOfFirst { it.familyId.key() == selectedFamily } + 1
                        val title =
                            if (child != null && familyNumber > 0) {
                                stringResource(R.string.family_with_child, familyNumber, child.name)
                            } else
                                stringResource(
                                    when (route) {
                                        TrackerDestination.TODAY -> R.string.nav_today
                                        TrackerDestination.HISTORY -> R.string.nav_history
                                        TrackerDestination.FAMILY -> R.string.nav_family
                                        TrackerDestination.CAPTURE -> R.string.add_activity
                                    }
                                )
                        val sharingActions =
                            trackerSharingActions(
                                context,
                                sharingState,
                                feedback,
                                scope,
                                sharing,
                                family,
                                recipientFamilies,
                                inviteOrigin,
                                { selectedFamily },
                                onInvitationConsumed,
                                recordRelayOrigin,
                                errorText,
                            )
                        val backupActions =
                            trackerBackupActions(
                                backupState,
                                feedback,
                                scope,
                                store,
                                sharing,
                                family,
                                activeShared,
                                readFile,
                                writeFile,
                                availableMemory,
                                recordSave,
                                { restored ->
                                    selectedFamily = restored.familyId.key()
                                    selectedChild = null
                                },
                            )
                        TrackerScaffold(
                            state =
                                TrackerChromeState(
                                    route,
                                    title,
                                    child != null,
                                    family != null && child != null,
                                ),
                            scrollState = scrollState,
                            snackbarHostState = snackbarHostState,
                            onNavigate = { destination = it },
                            onBack = {
                                destination = TrackerDestination.TODAY
                                captureKind = null
                            },
                            onSwitchTarget = { showTargetPicker = true },
                        ) {
                            if (route == TrackerDestination.FAMILY) {
                                FamilyScreen(
                                    state =
                                        FamilyUiState(
                                            family = family,
                                            child = child,
                                            families = families,
                                            children = children,
                                            selectedFamily = selectedFamily,
                                            selectedChild = selectedChild,
                                            familyChildNames = familyChildNames,
                                            removedFamilies = removedFamilies,
                                            activeShared = activeShared,
                                            activeFamilyIsLocal = activeFamilyIsLocal,
                                            automaticSyncDelayed = automaticSyncDelayed,
                                            automaticSyncBlocked = automaticSyncBlocked,
                                            shareStage = shareStage,
                                            activeSharedSnapshot = activeSharedSnapshot,
                                            activeUnusedInvitationIds = activeUnusedInvitationIds,
                                            deviceLabels = deviceLabels,
                                            showAccessControls = showAccessControls,
                                            showFamilySetup = showFamilySetup,
                                            showShareForm = showShareForm,
                                            shareInProgress = shareInProgress,
                                            relayOrigin = relayOrigin,
                                            relayPublicKey = relayPublicKey,
                                            inviteOrigin = inviteOrigin,
                                            inviteAsManager = inviteAsManager,
                                            inviteInProgress = inviteInProgress,
                                            invitationFragment = invitationFragment,
                                            joinFirst = joinFirst,
                                            showJoinForm = showJoinForm,
                                            recipientFamilies = recipientFamilies,
                                            readyRecipientKeys = readyRecipientKeys,
                                            selectedRecipient = selectedRecipient,
                                            receivedFragment = receivedFragment,
                                            joinInProgress = joinInProgress,
                                            sharedSnapshot = sharedSnapshot,
                                            joinStage = joinStage,
                                            restoredOrigin = restoredOrigin,
                                            ageLabel =
                                                child
                                                    ?.let {
                                                        childAgeLabel(context, it.birthDateString())
                                                    }
                                                    .orEmpty(),
                                            showChildDetails = showChildDetails,
                                            showDataControls = showDataControls,
                                            completed = completed,
                                            revision = revision,
                                            protectBackup = protectBackup,
                                            backupPassword = backupPassword,
                                            hasPendingRestore = pendingRestore != null,
                                            pendingRestoreProtected = pendingRestoreProtected,
                                            restorePassword = restorePassword,
                                            restoreInfo = restoreInfo,
                                        ),
                                    actions =
                                        FamilyActions(
                                            onOpenJoin = action@{ showJoinForm = true },
                                            onSelectRecipient = action@{ recipient ->
                                                    selectedRecipient = recipient.familyId.key()
                                                    joinStage = null
                                                },
                                            onReceivedFragmentChange = action@{ it ->
                                                    receivedFragment = it
                                                },
                                            onJoinOrRetry = sharingActions.onJoinOrRetry,
                                            onToggleAccess = action@{
                                                    showAccessControls = !showAccessControls
                                                },
                                            onNameDevice = action@{ target, snapshot ->
                                                    val key =
                                                        deviceLabelKey(
                                                            snapshot.family.familyId,
                                                            target,
                                                        )
                                                    deviceLabelTarget = key
                                                    deviceLabelDraft = deviceLabels[key].orEmpty()
                                                },
                                            onSyncShared = sharingActions.onSyncShared,
                                            onRemoveDevice = action@{ device ->
                                                    removalTarget = device.deviceId
                                                },
                                            onPromoteDevice = action@{ device, nextRole ->
                                                    val family = family ?: return@action
                                                    roleChangeTarget =
                                                        PendingRoleChange(
                                                            family,
                                                            device.deviceId.copyOf(),
                                                            nextRole,
                                                            activeFamilyIsLocal,
                                                        )
                                                },
                                            onRemovePendingDevice = action@{ pending ->
                                                    val family = family ?: return@action
                                                    pendingDeviceRemoval =
                                                        PendingDeviceRemoval(
                                                            family,
                                                            pending.invitationId.copyOf(),
                                                            pending.deviceId.copyOf(),
                                                            activeFamilyIsLocal,
                                                        )
                                                },
                                            onCancelInvitation = action@{ invitationId ->
                                                    val family = family ?: return@action
                                                    cancelInvitationTarget =
                                                        PendingInvitationCancel(
                                                            family,
                                                            invitationId.copyOf(),
                                                            activeFamilyIsLocal,
                                                        )
                                                },
                                            onInviteMember = action@{ inviteAsManager = false },
                                            onInviteManager = action@{ inviteAsManager = true },
                                            onCreateInvite = sharingActions.onCreateInvite,
                                            onShareAndroidInvitation = action@{ fragment ->
                                                    shareInvitation(
                                                        context,
                                                        invitationLink(fragment),
                                                    )
                                                },
                                            onCopyAndroidInvitation = action@{ fragment ->
                                                    copyInvitation(
                                                        context,
                                                        invitationLink(fragment),
                                                    )
                                                    message =
                                                        context.getString(
                                                            R.string.invitation_copied
                                                        )
                                                },
                                            onShareBrowserInvitation = action@{ fragment ->
                                                    shareInvitation(
                                                        context,
                                                        browserInvitationLink(
                                                            inviteOrigin,
                                                            fragment,
                                                        ),
                                                    )
                                                },
                                            onMakePrivateCopy = action@{
                                                    val family = family ?: return@action
                                                    scope.launch {
                                                        runCatching {
                                                                withContext(Dispatchers.IO) {
                                                                    sharing.privateCopy(
                                                                        family,
                                                                        System.currentTimeMillis(),
                                                                    )
                                                                }
                                                            }
                                                            .onSuccess { copy ->
                                                                selectedFamily = copy.familyId.key()
                                                                selectedChild = null
                                                                version++
                                                                message =
                                                                    context.getString(
                                                                        R.string
                                                                            .private_copy_created
                                                                    )
                                                            }
                                                            .onFailure { message = errorText }
                                                    }
                                                },
                                            onSelectFamily = action@{ item ->
                                                    selectedFamily = item.familyId.key()
                                                    selectedChild = null
                                                    showChildDetails = false
                                                    showAddChildForm = false
                                                    childName = ""
                                                    childBirthDate = ""
                                                    childSex = 3u.toUByte()
                                                },
                                            onContinueInPrivateCopy = action@{ source ->
                                                    scope.launch {
                                                        runCatching {
                                                                withContext(Dispatchers.IO) {
                                                                    sharing.privateCopy(
                                                                        source,
                                                                        System.currentTimeMillis(),
                                                                    )
                                                                }
                                                            }
                                                            .onSuccess { copy ->
                                                                selectedFamily = copy.familyId.key()
                                                                selectedChild = null
                                                                version++
                                                                message =
                                                                    context.getString(
                                                                        R.string
                                                                            .private_copy_created
                                                                    )
                                                            }
                                                            .onFailure { message = errorText }
                                                    }
                                                },
                                            onToggleFamilyOptions = action@{
                                                    showFamilySetup = !showFamilySetup
                                                },
                                            onNewFamily = action@{
                                                    scope.launch {
                                                        runCatching {
                                                                withContext(Dispatchers.IO) {
                                                                    store.createFamily(
                                                                        System.currentTimeMillis()
                                                                    )
                                                                }
                                                            }
                                                            .onSuccess { created ->
                                                                selectedFamily =
                                                                    created.familyId.key()
                                                                selectedChild = null
                                                                showFamilySetup = false
                                                                showChildDetails = false
                                                                showAddChildForm = false
                                                                childName = ""
                                                                childBirthDate = ""
                                                                childSex = 3u.toUByte()
                                                                version++
                                                                message = null
                                                            }
                                                            .onFailure { message = errorText }
                                                    }
                                                },
                                            onShareFamilyAction =
                                                sharingActions.onShareFamilyAction,
                                            onOpenSharingControls = action@{ showShareForm = true },
                                            onRelayOriginChange = action@{ it -> relayOrigin = it },
                                            onRelayPublicKeyChange = action@{ it ->
                                                    relayPublicKey = it
                                                },
                                            onShareRetryButton = sharingActions.onShareRetryButton,
                                            onSyncManager = sharingActions.onSyncManager,
                                            onSelectChild = action@{ item ->
                                                    selectedChild = item.id.key()
                                                    showChildDetails = false
                                                    showAddChildForm = false
                                                    childName = ""
                                                    childBirthDate = ""
                                                    childSex = 3u.toUByte()
                                                },
                                            onToggleChildOptions = action@{
                                                    showChildDetails = !showChildDetails
                                                },
                                            onEditChildProfile = action@{
                                                    val family = family ?: return@action
                                                    val child = child ?: return@action
                                                    val birthDate = child.birthDateString()
                                                    val sex = child.sex ?: 3u.toUByte()
                                                    pendingChildProfileEdit =
                                                        PendingChildProfileEdit(
                                                            family,
                                                            child.id.copyOf(),
                                                            activeShared,
                                                            child.name,
                                                            child.name,
                                                            birthDate,
                                                            birthDate,
                                                            sex,
                                                            sex,
                                                        )
                                                },
                                            onAddAnotherChild = action@{
                                                    showAddChildForm = true
                                                    childName = ""
                                                    childBirthDate = ""
                                                    childSex = 3u.toUByte()
                                                },
                                            onAddChild = action@{ showAddChildForm = true },
                                            onToggleData = action@{
                                                    showDataControls = !showDataControls
                                                },
                                            onToggleBackupProtection =
                                                backupActions.onToggleBackupProtection,
                                            onBackupPasswordChange =
                                                backupActions.onBackupPasswordChange,
                                            onSaveBackup = backupActions.onSaveBackup,
                                            onExportAnalysisCsv = backupActions.onExportAnalysisCsv,
                                            onRestoreBackup = backupActions.onRestoreBackup,
                                            onRestorePasswordChange =
                                                backupActions.onRestorePasswordChange,
                                            onInspectProtected = backupActions.onInspectProtected,
                                            onConfirmRestore = backupActions.onConfirmRestore,
                                        ),
                                )
                            }

                            if (family != null) {

                                if (child != null && route == TrackerDestination.TODAY) {
                                    TodayScreen(
                                        state =
                                            TodayUiState(
                                                activeShared = activeShared,
                                                ageLabel =
                                                    childAgeLabel(context, child.birthDateString()),
                                                automaticSyncDelayed = automaticSyncDelayed,
                                                automaticSyncBlocked = automaticSyncBlocked,
                                                summaryIsCurrent =
                                                    loadedChildKey == selectedChild &&
                                                        daySummaryDay ==
                                                            LocalDate.now(ZoneId.systemDefault()),
                                                daySummary = daySummary,
                                                entries = entries,
                                                entriesAreCurrent = loadedChildKey == selectedChild,
                                            ),
                                        actions =
                                            TodayActions(
                                                onStopSleep = action@{ timer ->
                                                        val end = System.currentTimeMillis()
                                                        val endOffset =
                                                            (TimeZone.getDefault().getOffset(end) /
                                                                    60_000)
                                                                .toShort()
                                                        change {
                                                            if (activeShared)
                                                                sharing.stopSleep(
                                                                    family,
                                                                    timer.childId,
                                                                    timer.id,
                                                                    end,
                                                                    endOffset,
                                                                )
                                                            else
                                                                store.stopSleep(
                                                                    family,
                                                                    timer.childId,
                                                                    timer.id,
                                                                    end,
                                                                    endOffset,
                                                                    end,
                                                                )
                                                        }
                                                    },
                                                onStartSleep = action@{
                                                        change(
                                                            onSaved = {
                                                                if (
                                                                    Build.VERSION.SDK_INT >= 33 &&
                                                                        context.checkSelfPermission(
                                                                            Manifest.permission
                                                                                .POST_NOTIFICATIONS
                                                                        ) !=
                                                                            PackageManager
                                                                                .PERMISSION_GRANTED
                                                                )
                                                                    notificationPermission.launch(
                                                                        Manifest.permission
                                                                            .POST_NOTIFICATIONS
                                                                    )
                                                            }
                                                        ) {
                                                            if (activeShared)
                                                                sharing.startSleepWithPlace(
                                                                    family,
                                                                    child.id,
                                                                    nowTime(),
                                                                    null,
                                                                )
                                                            else
                                                                store.startSleepWithPlace(
                                                                    family,
                                                                    child.id,
                                                                    nowTime(),
                                                                    null,
                                                                )
                                                        }
                                                    },
                                                onQuickWetDiaper = action@{
                                                        change {
                                                            val at = nowTime()
                                                            if (activeShared)
                                                                sharing.logDiaper(
                                                                    family,
                                                                    child.id,
                                                                    1u.toUByte(),
                                                                    at,
                                                                )
                                                            else
                                                                store.logDiaper(
                                                                    family,
                                                                    child.id,
                                                                    1u.toUByte(),
                                                                    at,
                                                                )
                                                        }
                                                    },
                                                onOpenBottle = action@{
                                                        captureKind = CaptureKind.BOTTLE
                                                        destination = TrackerDestination.CAPTURE
                                                    },
                                                onOpenDiaper = action@{
                                                        captureKind = CaptureKind.DIAPER
                                                        destination = TrackerDestination.CAPTURE
                                                    },
                                                onAddActivity = action@{
                                                        captureKind = null
                                                        destination = TrackerDestination.CAPTURE
                                                    },
                                                onViewTimeline = action@{
                                                        destination = TrackerDestination.HISTORY
                                                    },
                                            ),
                                    )
                                }
                                if (child != null && route == TrackerDestination.CAPTURE) {
                                    CaptureRoute(
                                        draft = captureDraft,
                                        captureKind = captureKind,
                                        family = family,
                                        child = child,
                                        activeShared = activeShared,
                                        store = store,
                                        sharing = sharing,
                                        scope = scope,
                                        feedback = feedback,
                                        errorText = errorText,
                                        performChange = { onSaved, action ->
                                            change(onSaved, action)
                                        },
                                        onSelectKind = { kind ->
                                            captureKind = kind
                                            scope.launch { captureScrollState.scrollTo(0) }
                                        },
                                        finishCapture = ::finishCapture,
                                        requestTimerNotification = {
                                            if (
                                                Build.VERSION.SDK_INT >= 33 &&
                                                    context.checkSelfPermission(
                                                        Manifest.permission.POST_NOTIFICATIONS
                                                    ) != PackageManager.PERMISSION_GRANTED
                                            )
                                                notificationPermission.launch(
                                                    Manifest.permission.POST_NOTIFICATIONS
                                                )
                                        },
                                    )
                                }
                                if (child != null && route == TrackerDestination.HISTORY) {
                                    HistoryScreen(
                                        state =
                                            HistoryUiState(
                                                selectedHistoryDay = selectedHistoryDay,
                                                timelineFilter = timelineFilter,
                                                entriesAreCurrent = loadedChildKey == selectedChild,
                                                entries = entries,
                                                expandedEntryKey = expandedEntryKey,
                                            ),
                                        actions =
                                            HistoryActions(
                                                onChooseHistoryDay = action@{
                                                        val day =
                                                            selectedHistoryDay?.let {
                                                                LocalDate.parse(it)
                                                            } ?: LocalDate.now()
                                                        DatePickerDialog(
                                                                context,
                                                                { _, year, month, date ->
                                                                    selectedHistoryDay =
                                                                        LocalDate.of(
                                                                                year,
                                                                                month + 1,
                                                                                date,
                                                                            )
                                                                            .toString()
                                                                    expandedEntryKey = null
                                                                },
                                                                day.year,
                                                                day.monthValue - 1,
                                                                day.dayOfMonth,
                                                            )
                                                            .apply {
                                                                datePicker.maxDate =
                                                                    System.currentTimeMillis()
                                                            }
                                                            .show()
                                                    },
                                                onShowAllDays = action@{
                                                        selectedHistoryDay = null
                                                        expandedEntryKey = null
                                                    },
                                                onTimelineFilterChange = action@{ filter ->
                                                        timelineFilter = filter
                                                    },
                                                onToggleEntryActions = action@{ entryKey ->
                                                        expandedEntryKey =
                                                            if (expandedEntryKey == entryKey) null
                                                            else entryKey
                                                    },
                                                onStopSleep = action@{ entry ->
                                                        val end = System.currentTimeMillis()
                                                        val endOffset =
                                                            (TimeZone.getDefault().getOffset(end) /
                                                                    60_000)
                                                                .toShort()
                                                        change {
                                                            if (activeShared)
                                                                sharing.stopSleep(
                                                                    family,
                                                                    entry.childId,
                                                                    entry.id,
                                                                    end,
                                                                    endOffset,
                                                                )
                                                            else
                                                                store.stopSleep(
                                                                    family,
                                                                    entry.childId,
                                                                    entry.id,
                                                                    end,
                                                                    endOffset,
                                                                    end,
                                                                )
                                                        }
                                                    },
                                                onEditSleep = action@{ entry ->
                                                        pendingSleepEdit =
                                                            PendingSleepEdit(
                                                                family,
                                                                entry.childId.copyOf(),
                                                                entry.id.copyOf(),
                                                                activeShared,
                                                                entry.startUtcMs,
                                                                ((entry.endUtcMs!! -
                                                                        entry.startUtcMs) / 60_000L)
                                                                    .toString(),
                                                            )
                                                    },
                                                onEditSleepPlace = action@{ entry ->
                                                        pendingSleepPlaceEdit =
                                                            PendingSleepPlaceEdit(
                                                                family,
                                                                entry.childId.copyOf(),
                                                                entry.id.copyOf(),
                                                                activeShared,
                                                                entry.sleepPlace,
                                                            )
                                                    },
                                                onAddActivityNote = action@{ entry ->
                                                        pendingNoteEdit =
                                                            PendingNoteEdit(
                                                                family,
                                                                entry.childId.copyOf(),
                                                                entry.id.copyOf(),
                                                                activeShared,
                                                                entry.note.orEmpty(),
                                                                entry.kind == "note",
                                                                entry.note != null,
                                                            )
                                                    },
                                                onEditEntryTime = action@{ entry ->
                                                        pendingTimeEdit =
                                                            PendingTimeEdit(
                                                                family,
                                                                entry.childId.copyOf(),
                                                                entry.id.copyOf(),
                                                                activeShared,
                                                                entry.startUtcMs,
                                                                entry.offsetMinutes,
                                                            )
                                                    },
                                                onMoveCompletedSession = action@{ entry ->
                                                        pendingTimeEdit =
                                                            PendingTimeEdit(
                                                                family,
                                                                entry.childId.copyOf(),
                                                                entry.id.copyOf(),
                                                                activeShared,
                                                                entry.startUtcMs,
                                                                entry.offsetMinutes,
                                                                intervalDurationMs =
                                                                    entry.endUtcMs!! -
                                                                        entry.startUtcMs,
                                                            )
                                                    },
                                                onEditBottle = action@{ entry ->
                                                        pendingBottleEdit =
                                                            PendingBottleEdit(
                                                                family,
                                                                entry.childId.copyOf(),
                                                                entry.id.copyOf(),
                                                                activeShared,
                                                                localizedEntered(
                                                                    context,
                                                                    entry.bottleEntered
                                                                        ?: entry.bottleMl.toString(),
                                                                ),
                                                                entry.bottleUnit ?: 1u.toUByte(),
                                                                entry.bottleContent ?: 4u.toUByte(),
                                                            )
                                                    },
                                                onEditBreast = action@{ entry ->
                                                        val savedSegments = entry.breastSegments!!
                                                        pendingBreastEdit =
                                                            PendingBreastEdit(
                                                                family,
                                                                entry.childId.copyOf(),
                                                                entry.id.copyOf(),
                                                                activeShared,
                                                                entry.startUtcMs,
                                                                entry.offsetMinutes,
                                                                savedSegments.map {
                                                                    it.side to
                                                                        ((it.endUtcMs -
                                                                                it.startUtcMs) /
                                                                                60_000L)
                                                                            .toString()
                                                                },
                                                                savedSegments.mapIndexed {
                                                                    index,
                                                                    segment ->
                                                                    if (index == 0) 0L
                                                                    else
                                                                        segment.startUtcMs -
                                                                            savedSegments[index - 1]
                                                                                .endUtcMs
                                                                },
                                                            )
                                                    },
                                                onEditDiaper = action@{ entry ->
                                                        pendingDiaperEdit =
                                                            PendingDiaperEdit(
                                                                family,
                                                                entry.childId.copyOf(),
                                                                entry.id.copyOf(),
                                                                activeShared,
                                                                entry.diaperKind!!,
                                                            )
                                                    },
                                                onEditSolids = action@{ entry ->
                                                        pendingSolidsEdit =
                                                            PendingSolidsEdit(
                                                                family,
                                                                entry.childId.copyOf(),
                                                                entry.id.copyOf(),
                                                                activeShared,
                                                                entry.solidsFoods!!.joinToString(
                                                                    "\n"
                                                                ),
                                                                entry.solidsAmount.orEmpty(),
                                                            )
                                                    },
                                                onEditPump = action@{ entry ->
                                                        pendingPumpEdit =
                                                            PendingPumpEdit(
                                                                family,
                                                                entry.childId.copyOf(),
                                                                entry.id.copyOf(),
                                                                activeShared,
                                                                entry.pumpLeftMl
                                                                    ?.toString()
                                                                    .orEmpty(),
                                                                entry.pumpRightMl
                                                                    ?.toString()
                                                                    .orEmpty(),
                                                                entry.pumpTotalMl
                                                                    ?.toString()
                                                                    .orEmpty(),
                                                            )
                                                    },
                                                onEditMedication = action@{ entry ->
                                                        pendingMedicationEdit =
                                                            PendingMedicationEdit(
                                                                family,
                                                                entry.childId.copyOf(),
                                                                entry.id.copyOf(),
                                                                activeShared,
                                                                entry.medicationName.orEmpty(),
                                                                entry.medicationDoseAmount
                                                                    .orEmpty(),
                                                                entry.medicationDoseUnit.orEmpty(),
                                                            )
                                                    },
                                                onEditGrowth = action@{ entry ->
                                                        pendingGrowthEdit =
                                                            PendingGrowthEdit(
                                                                family,
                                                                entry.childId.copyOf(),
                                                                entry.id.copyOf(),
                                                                activeShared,
                                                                localizedEntered(
                                                                    context,
                                                                    entry.growthWeightEntered
                                                                        ?: entry.growthWeightG
                                                                            ?.toString()
                                                                            .orEmpty(),
                                                                ),
                                                                entry.growthWeightUnit
                                                                    ?: 10u.toUByte(),
                                                                localizedEntered(
                                                                    context,
                                                                    entry.growthLengthEntered
                                                                        ?: entry.growthLengthMm
                                                                            ?.toString()
                                                                            .orEmpty(),
                                                                ),
                                                                entry.growthLengthUnit
                                                                    ?: 20u.toUByte(),
                                                                localizedEntered(
                                                                    context,
                                                                    entry.growthHeadEntered
                                                                        ?: entry.growthHeadMm
                                                                            ?.toString()
                                                                            .orEmpty(),
                                                                ),
                                                                entry.growthHeadUnit
                                                                    ?: 20u.toUByte(),
                                                            )
                                                    },
                                                onEditTemperature = action@{ entry ->
                                                        pendingTemperatureEdit =
                                                            PendingTemperatureEdit(
                                                                family,
                                                                entry.childId.copyOf(),
                                                                entry.id.copyOf(),
                                                                activeShared,
                                                                localizedEntered(
                                                                    context,
                                                                    entry.temperatureEntered
                                                                        ?: entry.temperatureC!!,
                                                                ),
                                                                entry.temperatureUnit
                                                                    ?: 30u.toUByte(),
                                                            )
                                                    },
                                                onDeleteEntry = action@{ entry ->
                                                        pendingDelete =
                                                            PendingActivityDelete(
                                                                family,
                                                                entry.childId.copyOf(),
                                                                entry.id.copyOf(),
                                                                activeShared,
                                                            )
                                                    },
                                            ),
                                    )
                                }
                            }

                            message?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                        }
                        if (showAddChildForm && family != null)
                            ChildProfileScreen(
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
                                        val birthDay =
                                            runCatching {
                                                    childBirthDate
                                                        .takeIf { it.isNotBlank() }
                                                        ?.let { LocalDate.parse(it).toEpochDay() }
                                                }
                                                .getOrElse {
                                                    message =
                                                        context.getString(
                                                            R.string.birth_date_invalid
                                                        )
                                                    return@save
                                                }
                                        childProfileSaving = true
                                        scope.launch {
                                            runCatching {
                                                    withContext(Dispatchers.IO) {
                                                        if (activeShared)
                                                            sharing.addChildWithMetadata(
                                                                family,
                                                                name,
                                                                birthDay,
                                                                childSex,
                                                                System.currentTimeMillis(),
                                                            )
                                                        else
                                                            store.addChildWithMetadata(
                                                                family,
                                                                name,
                                                                birthDay,
                                                                childSex,
                                                                System.currentTimeMillis(),
                                                            )
                                                    }
                                                }
                                                .onSuccess { created ->
                                                    selectedChild = created.key()
                                                    showAddChildForm = false
                                                    showChildDetails = false
                                                    childName = ""
                                                    childBirthDate = ""
                                                    childSex = 3u.toUByte()
                                                    version++
                                                    message = null
                                                }
                                                .onFailure { message = errorText }
                                            childProfileSaving = false
                                        }
                                    },
                            )
                        if (showTargetPicker) {
                            AlertDialog(
                                onDismissRequest = { showTargetPicker = false },
                                title = { Text(stringResource(R.string.switch_target)) },
                                text = {
                                    Column(
                                        Modifier.verticalScroll(rememberScrollState()),
                                        verticalArrangement = Arrangement.spacedBy(8.dp),
                                    ) {
                                        Text(
                                            stringResource(R.string.families),
                                            style = MaterialTheme.typography.titleMedium,
                                        )
                                        families.forEachIndexed { index, item ->
                                            val name = familyChildNames[item.familyId.key()]
                                            val label =
                                                if (name == null)
                                                    stringResource(
                                                        R.string.family_number,
                                                        index + 1,
                                                    )
                                                else
                                                    stringResource(
                                                        R.string.family_with_child,
                                                        index + 1,
                                                        name,
                                                    )
                                            FilterChip(
                                                selected = item.familyId.key() == selectedFamily,
                                                onClick = {
                                                    selectedFamily = item.familyId.key()
                                                    selectedChild = null
                                                    showTargetPicker = false
                                                },
                                                label = { Text(label) },
                                            )
                                        }
                                        if (children.isNotEmpty()) {
                                            Text(
                                                stringResource(R.string.children),
                                                style = MaterialTheme.typography.titleMedium,
                                            )
                                            children.forEach { item ->
                                                FilterChip(
                                                    selected = item.id.key() == selectedChild,
                                                    onClick = {
                                                        selectedChild = item.id.key()
                                                        showTargetPicker = false
                                                    },
                                                    label = { Text(item.name) },
                                                )
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
                                title = {
                                    Text(stringResource(R.string.history_removed_copy_title))
                                },
                                text = {
                                    Text(
                                        stringResource(
                                            R.string.history_removed_copy_destination,
                                            notice.sourceKey.take(8),
                                            notice.copy.familyId.key().take(8),
                                        )
                                    )
                                },
                                confirmButton = {
                                    Button(
                                        onClick = {
                                            val copyKey = notice.copy.familyId.key()
                                            if (
                                                selectionPrefs
                                                    .edit()
                                                    .putString("family", copyKey)
                                                    .remove("child")
                                                    .commit() &&
                                                    removalNoticePrefs
                                                        .edit()
                                                        .putString(notice.sourceKey, copyKey)
                                                        .commit()
                                            ) {
                                                selectedFamily = copyKey
                                                selectedChild = null
                                                pendingRemovalNotice = null
                                                version++
                                            } else message = errorText
                                        }
                                    ) {
                                        Text(stringResource(R.string.continue_in_private_copy))
                                    }
                                },
                            )
                        }
                        EntryEditController(edits, store, sharing, scope, feedback, errorText) {
                            onSaved,
                            action ->
                            change(onSaved, action)
                        }
                        cancelInvitationTarget?.let { target ->
                            AlertDialog(
                                onDismissRequest = { cancelInvitationTarget = null },
                                title = { Text(stringResource(R.string.cancel_invitation_title)) },
                                text = {
                                    Text(
                                        stringResource(
                                            R.string.cancel_invitation_warning,
                                            target.invitationId.key(),
                                        )
                                    )
                                },
                                confirmButton = {
                                    Button(
                                        onClick = {
                                            cancelInvitationTarget = null
                                            scope.launch {
                                                runCatching {
                                                        withContext(Dispatchers.IO) {
                                                            val origin =
                                                                if (target.localManager)
                                                                    lastRelayOrigin(target.family)
                                                                        ?: error(
                                                                            "Relay origin unavailable"
                                                                        )
                                                                else
                                                                    sharing.recipientOrigin(
                                                                        target.family
                                                                    )
                                                            sharing.cancelInvitation(
                                                                target.family,
                                                                origin,
                                                                target.invitationId,
                                                            )
                                                        }
                                                    }
                                                    .onSuccess {
                                                        invitationFragment = null
                                                        version++
                                                        message =
                                                            context.getString(
                                                                R.string.invitation_canceled
                                                            )
                                                    }
                                                    .onFailure { message = errorText }
                                            }
                                        }
                                    ) {
                                        Text(stringResource(R.string.confirm_cancel_invitation))
                                    }
                                },
                                dismissButton = {
                                    OutlinedButton(onClick = { cancelInvitationTarget = null }) {
                                        Text(stringResource(R.string.cancel))
                                    }
                                },
                            )
                        }
                        pendingDeviceRemoval?.let { target ->
                            val label =
                                deviceLabels[
                                        deviceLabelKey(target.family.familyId, target.deviceId)]
                                    ?.takeIf { it.isNotBlank() } ?: target.deviceId.key().take(8)
                            AlertDialog(
                                onDismissRequest = { pendingDeviceRemoval = null },
                                title = {
                                    Text(stringResource(R.string.remove_pending_device_title))
                                },
                                text = {
                                    Text(
                                        stringResource(
                                            R.string.remove_pending_device_warning,
                                            label,
                                        )
                                    )
                                },
                                confirmButton = {
                                    Button(
                                        onClick = {
                                            pendingDeviceRemoval = null
                                            scope.launch {
                                                runCatching {
                                                        withContext(Dispatchers.IO) {
                                                            val origin =
                                                                if (target.localManager)
                                                                    lastRelayOrigin(target.family)
                                                                        ?: error(
                                                                            "Relay origin unavailable"
                                                                        )
                                                                else
                                                                    sharing.recipientOrigin(
                                                                        target.family
                                                                    )
                                                            sharing.removePendingDevice(
                                                                target.family,
                                                                origin,
                                                                target.invitationId,
                                                                target.deviceId,
                                                            )
                                                        }
                                                    }
                                                    .onSuccess {
                                                        screenData =
                                                            screenData?.copy(
                                                                mainSharedSnapshot = it
                                                            )
                                                        version++
                                                        message =
                                                            context.getString(
                                                                R.string.pending_device_removed
                                                            )
                                                    }
                                                    .onFailure { message = errorText }
                                            }
                                        }
                                    ) {
                                        Text(stringResource(R.string.confirm_remove_pending_device))
                                    }
                                },
                                dismissButton = {
                                    OutlinedButton(onClick = { pendingDeviceRemoval = null }) {
                                        Text(stringResource(R.string.cancel))
                                    }
                                },
                            )
                        }
                        roleChangeTarget?.let { target ->
                            val label =
                                deviceLabels[
                                        deviceLabelKey(target.family.familyId, target.targetId)]
                                    ?.takeIf { it.isNotBlank() } ?: target.targetId.key().take(8)
                            AlertDialog(
                                onDismissRequest = { roleChangeTarget = null },
                                title = { Text(stringResource(R.string.change_device_role_title)) },
                                text = {
                                    Text(
                                        stringResource(
                                            R.string.change_device_role_warning,
                                            label,
                                            if (target.newRole == 2.toUByte())
                                                stringResource(R.string.manager_role)
                                            else stringResource(R.string.member_role),
                                        )
                                    )
                                },
                                confirmButton = {
                                    Button(
                                        onClick = {
                                            roleChangeTarget = null
                                            scope.launch {
                                                runCatching {
                                                        withContext(Dispatchers.IO) {
                                                            val origin =
                                                                if (target.localManager)
                                                                    lastRelayOrigin(target.family)
                                                                        ?: error(
                                                                            "Relay origin unavailable"
                                                                        )
                                                                else
                                                                    sharing.recipientOrigin(
                                                                        target.family
                                                                    )
                                                            sharing.changeDeviceRole(
                                                                target.family,
                                                                origin,
                                                                target.targetId,
                                                                target.newRole,
                                                            )
                                                        }
                                                    }
                                                    .onSuccess {
                                                        screenData =
                                                            screenData?.copy(
                                                                mainSharedSnapshot = it
                                                            )
                                                        version++
                                                        message =
                                                            context.getString(
                                                                R.string.device_role_changed
                                                            )
                                                    }
                                                    .onFailure { message = errorText }
                                            }
                                        }
                                    ) {
                                        Text(stringResource(R.string.confirm_device_role_change))
                                    }
                                },
                                dismissButton = {
                                    OutlinedButton(onClick = { roleChangeTarget = null }) {
                                        Text(stringResource(R.string.cancel))
                                    }
                                },
                            )
                        }
                        removalTarget?.let { target ->
                            val targetLabel =
                                family
                                    ?.let { deviceLabels[deviceLabelKey(it.familyId, target)] }
                                    ?.takeIf { it.isNotBlank() }
                            val targetDescription =
                                if (targetLabel == null) target.key()
                                else
                                    context.getString(
                                        R.string.named_device_id,
                                        targetLabel,
                                        target.key(),
                                    )
                            AlertDialog(
                                onDismissRequest = { removalTarget = null },
                                title = { Text(stringResource(R.string.remove_device_title)) },
                                text = {
                                    Text(
                                        stringResource(
                                            R.string.remove_device_warning,
                                            targetDescription,
                                        )
                                    )
                                },
                                confirmButton = {
                                    Button(
                                        onClick = {
                                            removalTarget = null
                                            val chosen = family ?: return@Button
                                            scope.launch {
                                                runCatching {
                                                        withContext(Dispatchers.IO) {
                                                            sharing.removeDevice(
                                                                chosen,
                                                                relayOrigin.trim(),
                                                                target,
                                                            )
                                                        }
                                                    }
                                                    .onSuccess {
                                                        screenData =
                                                            screenData?.copy(
                                                                mainSharedSnapshot = it
                                                            )
                                                        version++
                                                        message =
                                                            context.getString(
                                                                R.string.device_removed
                                                            )
                                                    }
                                                    .onFailure { message = errorText }
                                            }
                                        }
                                    ) {
                                        Text(stringResource(R.string.confirm_remove_device))
                                    }
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
                                        label = {
                                            Text(stringResource(R.string.device_label_hint))
                                        },
                                        singleLine = true,
                                    )
                                },
                                confirmButton = {
                                    Button(
                                        onClick = {
                                            val label = deviceLabelDraft.trim()
                                            if (label.isEmpty()) {
                                                deviceLabelPrefs.edit().remove(key).apply()
                                                deviceLabels = deviceLabels - key
                                            } else {
                                                deviceLabelPrefs
                                                    .edit()
                                                    .putString(key, label)
                                                    .apply()
                                                deviceLabels = deviceLabels + (key to label)
                                            }
                                            deviceLabelTarget = null
                                        }
                                    ) {
                                        Text(stringResource(R.string.save_changes))
                                    }
                                },
                                dismissButton = {
                                    OutlinedButton(onClick = { deviceLabelTarget = null }) {
                                        Text(stringResource(R.string.cancel))
                                    }
                                },
                            )
                        }
                    }
                }
            }
        }
    }
}

internal fun nowTime(): ActivityWhen {
    return activityWhen(System.currentTimeMillis())
}

internal fun activityWhen(atMs: Long): ActivityWhen =
    ActivityWhen(
        atMs,
        (TimeZone.getDefault().getOffset(atMs) / 60_000).toShort(),
        System.currentTimeMillis(),
    )

internal fun savedTime(utcMs: Long): String =
    DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(utcMs))
