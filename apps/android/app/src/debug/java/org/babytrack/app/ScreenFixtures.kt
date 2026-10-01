package org.babytrack.app

import androidx.compose.foundation.ScrollState
import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import java.time.Instant
import java.time.LocalDate
import uniffi.babytrack_core_ffi.*

/** Fictional rendering inputs only. Never open native storage or contact a relay. */
internal object ScreenFixtures {
    val nowMs = Instant.parse("2026-09-29T14:00:00Z").toEpochMilli()

    private fun id(seed: Int) = ByteArray(16) { (seed + it).toByte() }

    val family = FamilyRef(id(1), id(20))
    val child = ChildRow(id(40), "Rowan", LocalDate.parse("2026-06-29").toEpochDay(), 3u)

    private fun entry(seed: Int, kind: String, minutesAgo: Long): ActivityRow =
        ActivityRow(
            id = id(seed),
            childId = child.id,
            kind = kind,
            startUtcMs = nowMs - minutesAgo * 60_000L,
            offsetMinutes = 0,
            endUtcMs = null,
            sleepPlace = null,
            note = null,
            diaperKind = null,
            bottleMl = null,
            bottleEntered = null,
            bottleUnit = null,
            bottleContent = null,
            breastSide = null,
            breastSegments = null,
            solidsFoods = null,
            solidsAmount = null,
            pumpLeftMl = null,
            pumpRightMl = null,
            pumpTotalMl = null,
            growthWeightG = null,
            growthWeightEntered = null,
            growthWeightUnit = null,
            growthLengthMm = null,
            growthLengthEntered = null,
            growthLengthUnit = null,
            growthHeadMm = null,
            growthHeadEntered = null,
            growthHeadUnit = null,
            temperatureC = null,
            temperatureEntered = null,
            temperatureUnit = null,
            medicationName = null,
            medicationDoseAmount = null,
            medicationDoseUnit = null,
        )

    val typicalEntries =
        listOf(
            entry(60, "feed.bottle", 25)
                .copy(bottleMl = 120, bottleEntered = "120", bottleUnit = 1u, bottleContent = 2u),
            entry(61, "diaper", 65).copy(diaperKind = 1u),
            entry(62, "sleep", 150).copy(endUtcMs = nowMs - 70 * 60_000L, sleepPlace = 1u),
            entry(63, "note", 190).copy(note = "A quiet walk in the park."),
            entry(64, "feed.breast", 240).copy(endUtcMs = nowMs - 225 * 60_000L, breastSide = 1u),
            entry(65, "pump", 280).copy(endUtcMs = nowMs - 265 * 60_000L, pumpTotalMl = 90),
            entry(66, "feed.solids", 310)
                .copy(solidsFoods = listOf("Pear", "Oats"), solidsAmount = "A few spoonfuls"),
            entry(67, "growth", 350)
                .copy(
                    growthWeightG = 6200,
                    growthWeightEntered = "6.2",
                    growthWeightUnit = 11u,
                    growthLengthMm = 610,
                    growthLengthEntered = "61",
                    growthLengthUnit = 21u,
                ),
            entry(68, "temperature", 380)
                .copy(temperatureC = "36.8", temperatureEntered = "36.8", temperatureUnit = 30u),
            entry(69, "medication", 400)
                .copy(
                    medicationName = "Example only",
                    medicationDoseAmount = "1",
                    medicationDoseUnit = "unit",
                ),
            entry(70, "future.example", 450),
        )
    val runningSleep = entry(71, "sleep", 20).copy(sleepPlace = 1u)
    val nursingTimer =
        listOf(
            TimedSegment(1u, nowMs - 9 * 60_000L, nowMs - 5 * 60_000L - 26_000L),
            TimedSegment(2u, nowMs - 5 * 60_000L, null),
        )
    val summary = DaySummaryRow(80uL * 60_000uL, 3uL, 120uL, 1uL, 1uL, 0uL)

    fun capture() =
        CaptureUiState(
            childName = "Rowan",
            captureKind = null,
            logAtMs = null,
            amount = "",
            bottleUnit = 1u,
            bottleContent = 2u,
            breastMinutes = "",
            breastSide = 1u,
            breastDraftSegments = emptyList(),
            pumpMinutes = "",
            pumpLeft = "",
            pumpRight = "",
            pumpTotal = "",
            solidsFoods = "",
            solidsAmount = "",
            sleepMinutes = "",
            sleepPlace = null,
            growthWeight = "",
            growthWeightUnit = 11u,
            growthLength = "",
            growthLengthUnit = 21u,
            growthHead = "",
            growthHeadUnit = 21u,
            temperatureEntered = "",
            temperatureUnit = 30u,
            medicationName = "",
            doseAmount = "",
            doseUnit = "",
            noteText = "",
            nowMs = nowMs,
        )

    fun today() =
        TodayUiState(
                activeShared = false,
                ageLabel = "3 months",
                automaticSyncDelayed = false,
                automaticSyncBlocked = false,
                summaryIsCurrent = true,
                daySummary = null,
                entries = emptyList(),
                entriesAreCurrent = true,
                nowMs = nowMs,
            )
            .copy(daySummary = summary, entries = typicalEntries)

    fun history() =
        HistoryUiState(
                selectedHistoryDay = null,
                timelineFilter = TimelineFilter.ALL,
                entriesAreCurrent = true,
                entries = emptyList(),
                expandedEntryKey = null,
                daySummary = summary,
                nowMs = nowMs,
            )
            .copy(entries = typicalEntries)

    fun family() =
        FamilyUiState(
            family = family,
            child = child,
            families = listOf(family),
            children = listOf(child),
            selectedFamily = family.familyId.key(),
            selectedChild = child.id.key(),
            familyChildNames = emptyMap(),
            removedFamilies = emptyList(),
            activeShared = false,
            activeFamilyIsLocal = true,
            automaticSyncDelayed = false,
            automaticSyncBlocked = false,
            shareStage = null,
            activeSharedSnapshot = null,
            activeUnusedInvitationIds = null,
            deviceLabels = emptyMap(),
            showAccessControls = false,
            showFamilySetup = false,
            showShareForm = false,
            shareInProgress = false,
            relayOrigin = "",
            relayPublicKey = "",
            inviteOrigin = "",
            inviteAsManager = false,
            inviteInProgress = false,
            invitationFragment = null,
            joinFirst = false,
            showJoinForm = false,
            recipientFamilies = emptyList(),
            readyRecipientKeys = emptySet(),
            selectedRecipient = null,
            receivedFragment = "",
            joinInProgress = false,
            sharedSnapshot = null,
            joinStage = null,
            restoredOrigin = null,
            ageLabel = "3 months",
            showChildDetails = false,
            showDataControls = false,
            completed = null,
            revision = 0uL,
            protectBackup = false,
            backupPassword = "",
            hasPendingRestore = false,
            pendingRestoreProtected = false,
            restorePassword = "",
            restoreInfo = null,
        )

    private val sharedSnapshot =
        SharedSnapshotRow(
            family,
            12uL,
            listOf(child),
            typicalEntries,
            2uL,
            0uL,
            emptyList(),
            listOf(SharedDeviceRow(family.deviceId, 2u), SharedDeviceRow(id(90), 1u)),
            emptyList(),
        )
    private val filledCapture =
        capture()
            .copy(
                logAtMs = nowMs - 10 * 60_000L,
                amount = "120",
                breastMinutes = "12",
                breastDraftSegments = listOf(1u.toUByte() to 8L),
                pumpMinutes = "15",
                pumpTotal = "90",
                solidsFoods = "Pear\nOats",
                solidsAmount = "A few spoonfuls",
                sleepMinutes = "80",
                sleepPlace = 1u,
                growthWeight = "6.2",
                growthLength = "61",
                growthHead = "40",
                temperatureEntered = "36.8",
                noteText = "A quiet walk in the park.",
                medicationName = "Example only",
                doseAmount = "1",
                doseUnit = "unit",
                diaperKind = 3u,
                lastBottle = "120" to 1u.toUByte(),
                breastTimerMode = false,
            )
    val cases: List<ScreenFixture> =
        listOf(
            ScreenFixture(
                "today-empty",
                "Today · empty",
                today =
                    today()
                        .copy(
                            entries = emptyList(),
                            daySummary = DaySummaryRow(0uL, 0uL, 0uL, 0uL, 0uL, 0uL),
                        ),
            ),
            ScreenFixture("today-typical", "Today · typical day", today = today()),
            ScreenFixture(
                "today-timer",
                "Today · sleep running",
                today = today().copy(entries = listOf(runningSleep) + typicalEntries),
            ),
            ScreenFixture(
                "today-nursing",
                "Today · nursing timer running",
                today = today().copy(nursingSegments = nursingTimer),
            ),
            ScreenFixture(
                "capture-breast-timer",
                "Breast feed · timer running",
                capture =
                    capture()
                        .copy(
                            captureKind = CaptureKind.BREAST,
                            nursingSegments = nursingTimer,
                            lastBreastSide = 2u,
                        ),
            ),
            ScreenFixture(
                "capture-breast-idle",
                "Breast feed · timer ready",
                capture = capture().copy(captureKind = CaptureKind.BREAST, lastBreastSide = 2u),
            ),
            ScreenFixture(
                "capture-pump-timer",
                "Pumping · stopwatch running",
                capture =
                    capture()
                        .copy(captureKind = CaptureKind.PUMP, pumpTimerStartMs = nowMs - 7 * 60_000L - 12_000L),
            ),
            ScreenFixture(
                "today-sync-pending",
                "Today · sync pending",
                today = today().copy(activeShared = true, automaticSyncDelayed = true),
            ),
            ScreenFixture(
                "today-blocked",
                "Today · upload blocked",
                today = today().copy(activeShared = true, automaticSyncBlocked = true),
            ),
            ScreenFixture(
                "history-empty",
                "History · empty",
                history = history().copy(entries = emptyList()),
            ),
            ScreenFixture("history-typical", "History · today", history = history()),
            ScreenFixture(
                "history-all-days",
                "History · all days",
                history =
                    history()
                        .copy(
                            allDays = true,
                            entries =
                                typicalEntries +
                                    typicalEntries.take(3).mapIndexed { index, entry ->
                                        entry.copy(
                                            id = ByteArray(16) { (120 + index + it).toByte() },
                                            startUtcMs = entry.startUtcMs - 24 * 60 * 60_000L,
                                            endUtcMs = entry.endUtcMs?.minus(24 * 60 * 60_000L),
                                        )
                                    },
                        ),
            ),
            ScreenFixture(
                "history-filtered",
                "History · day and feed filter",
                history =
                    history()
                        .copy(
                            selectedHistoryDay = "2026-09-28",
                            timelineFilter = TimelineFilter.FEEDS,
                            daySummary = null,
                        ),
            ),
            ScreenFixture(
                "history-entry-actions",
                "History · entry actions",
                history = history().copy(expandedEntryKey = typicalEntries.first().id.key()),
            ),
            ScreenFixture("onboarding-welcome", "Onboarding · welcome", welcome = true),
            ScreenFixture("onboarding-profile", "Onboarding · child profile",
                profile = ChildProfileUiState(false, "", "", 3u, false, true, "", onboarding = true)),
            ScreenFixture("onboarding-ready", "Onboarding · ready to track",
                profile = ChildProfileUiState(false, "Rowan", "", 3u, false, true, "", onboarding = true)),
            ScreenFixture("onboarding-saving", "Onboarding · setting up",
                profile = ChildProfileUiState(false, "Rowan", "2026-06-29", 3u, true, true, "", onboarding = true)),
            ScreenFixture("onboarding-retry", "Onboarding · retry after interrupted setup",
                profile = ChildProfileUiState(false, "Rowan", "", 3u, false, true, "", onboarding = true,
                    errorMessage = "Couldn’t finish setup. Your details are saved here. Please try again.")),
            ScreenFixture("family-local", "Family · local", family = family()),
            ScreenFixture(
                "family-options",
                "Family · children and data",
                family =
                    family()
                        .copy(
                            showChildDetails = true,
                            showDataControls = true,
                            children = listOf(child, child.copy(id = id(80), name = "Skyler")),
                            protectBackup = true,
                        ),
            ),
            ScreenFixture(
                "family-shared-pending",
                "Family · shared with pending edits",
                family =
                    family()
                        .copy(
                            activeShared = true,
                            activeSharedSnapshot = sharedSnapshot,
                            showAccessControls = true,
                            inviteOrigin = "https://relay.invalid",
                            deviceLabels =
                                mapOf(deviceLabelKey(family.familyId, id(90)) to "Second phone"),
                        ),
            ),
            ScreenFixture(
                "family-join-pending",
                "Family · join pending",
                family =
                    family()
                        .copy(
                            showJoinForm = true,
                            recipientFamilies = listOf(FamilyRef(id(95), id(96))),
                            selectedRecipient = id(95).key(),
                            joinStage = "Waiting for a verified grant.",
                        ),
            ),
            ScreenFixture(
                "family-removed",
                "Family · verified access ended",
                family = family().copy(removedFamilies = listOf(FamilyRef(id(100), id(101)))),
            ),
            ScreenFixture("capture-chooser", "Add activity · all types", capture = capture()),
            ScreenFixture(
                "capture-bottle-empty",
                "Bottle · empty draft",
                capture = capture().copy(captureKind = CaptureKind.BOTTLE),
            ),
            ScreenFixture(
                "capture-bottle-invalid",
                "Bottle · invalid amount",
                capture = filledCapture.copy(captureKind = CaptureKind.BOTTLE, amount = "0"),
            ),
            ScreenFixture(
                "capture-note-long-name",
                "Note · long child name",
                capture =
                    filledCapture.copy(
                        captureKind = CaptureKind.NOTE,
                        childName = "Rowan Skyler Avery Morgan",
                    ),
            ),
            ScreenFixture(
                "child-create",
                "Child profile · create",
                profile = ChildProfileUiState(false, "", "", 3u, false, true, "Age unknown"),
            ),
            ScreenFixture(
                "child-edit",
                "Child profile · edit",
                profile =
                    ChildProfileUiState(true, "Rowan", "2026-06-29", 3u, false, false, "3 months"),
            ),
        ) +
            CaptureKind.entries.map { kind ->
                ScreenFixture(
                    "capture-${kind.name.lowercase()}",
                    "Capture · ${kind.name.lowercase().replaceFirstChar { it.uppercase() }}",
                    capture = filledCapture.copy(captureKind = kind),
                )
            }
}

internal data class ScreenFixture(
    val id: String,
    val label: String,
    val today: TodayUiState? = null,
    val history: HistoryUiState? = null,
    val family: FamilyUiState? = null,
    val capture: CaptureUiState? = null,
    val profile: ChildProfileUiState? = null,
    val welcome: Boolean = false,
) {
    val route: TrackerDestination
        get() =
            when {
                today != null -> TrackerDestination.TODAY
                history != null -> TrackerDestination.HISTORY
                capture != null -> TrackerDestination.CAPTURE
                else -> TrackerDestination.FAMILY
            }
}

@Composable
internal fun FixtureScreen(fixture: ScreenFixture, scrollState: ScrollState) {
    if (fixture.welcome) {
        OnboardingWelcomeScreen(scrollState = scrollState)
        return
    }
    fixture.profile?.let {
        ChildProfileContent(it, ChildProfileActions(), scrollState)
        return
    }
    val hasChild = fixture.family?.child != null || fixture.route != TrackerDestination.FAMILY
    val capture = fixture.capture
    val title =
        when {
            capture != null -> stringResource(capture.captureKind?.label ?: R.string.add_activity)
            hasChild -> ScreenFixtures.child.name
            else -> stringResource(R.string.nav_family)
        }
    TrackerScaffold(
        TrackerChromeState(
            fixture.route,
            title,
            hasChild,
            hasChild,
            subtitle =
                capture?.let { stringResource(R.string.capture_for_child, it.childName) },
            titleKind = capture?.captureKind?.activityKind,
        ),
        scrollState,
        bottomAction =
            capture?.takeIf { it.captureKind != null }?.let { { CaptureSaveActions(it, CaptureActions()) } },
    ) {
        fixture.today?.let { TodayScreen(it, TodayActions()) }
        fixture.history?.let { HistoryScreen(it, HistoryActions()) }
        fixture.family?.let { FamilyScreen(it, FamilyActions()) }
        fixture.capture?.let { CaptureScreen(it, CaptureActions()) }
    }
}
