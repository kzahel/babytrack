package org.babytrack.app

import android.app.Application
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import org.junit.After
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assume.assumeFalse
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.android.controller.ActivityController
import org.robolectric.annotation.Config

/** FS20/FS77 and FS30 against the actual release variant, not a simulated flag. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], application = Application::class)
class ReleaseFamilySafetyTest {
    @get:Rule val compose = createEmptyComposeRule()
    private var activity: ActivityController<ComponentActivity>? = null

    @Before fun requireRelease() { assumeFalse(BuildConfig.DEBUG) }
    @After fun closeActivity() { activity?.pause()?.stop()?.destroy() }

    private fun render(state: FamilyUiState, actions: FamilyActions = FamilyActions()) {
        activity = Robolectric.buildActivity(ComponentActivity::class.java).setup()
        activity!!.get().setContent {
            BabytrackTheme {
                Column(Modifier.verticalScroll(rememberScrollState())) {
                    FamilyScreen(state, actions)
                }
            }
        }
    }

    private fun shared() = ScreenFixtures.cases.single { it.id == "family-shared-pending" }.family!!

    @Test fun managerCanCancelAnUnusedLinkAndChangeDeviceRole() {
        val invitation = ByteArray(16) { 7 }
        var canceled: ByteArray? = null
        var promoted: UByte? = null
        render(shared().copy(activeUnusedInvitationIds = listOf(invitation)), FamilyActions(
            onCancelInvitation = { canceled = it },
            onPromoteDevice = { _, role -> promoted = role },
        ))
        compose.onNodeWithText("Make Second phone a manager").performScrollTo().performClick()
        assertEquals(2.toUByte(), promoted)
        compose.onNodeWithText("Cancel invitation 07070707").performScrollTo().performClick()
        assertArrayEquals(invitation, canceled)
    }

    @Test fun managerCanDemoteAnotherManager() {
        val state = shared()
        val snapshot = state.activeSharedSnapshot!!
        var demoted: UByte? = null
        render(state.copy(activeSharedSnapshot = snapshot.copy(devices = snapshot.devices.map {
            it.copy(role = 2u)
        })), FamilyActions(onPromoteDevice = { _, role -> demoted = role }))
        compose.onNodeWithText("Make Second phone a member").performScrollTo().performClick()
        assertEquals(1.toUByte(), demoted)
    }

    @Test fun memberDoesNotReceiveManagerControls() {
        val state = shared()
        val snapshot = state.activeSharedSnapshot!!
        render(state.copy(activeUnusedInvitationIds = listOf(ByteArray(16) { 7 }),
            activeSharedSnapshot = snapshot.copy(devices = snapshot.devices.map {
                if (it.deviceId.contentEquals(state.family!!.deviceId)) it.copy(role = 1u) else it
            })))
        compose.onNodeWithText("Make Second phone a manager").assertDoesNotExist()
        compose.onNodeWithText("Cancel invitation 07070707").assertDoesNotExist()
    }

    @Test fun noLocalBackupIsExplicit() {
        render(ScreenFixtures.family().copy(showDataControls = true))
        compose.onNodeWithText("No completed backup is known on this device.").performScrollTo()
    }

    @Test fun noSharedBackupIsExplicit() {
        render(shared().copy(showDataControls = true))
        compose.onNodeWithText("No completed backup is known on this device.").performScrollTo()
    }

    @Test fun localEditsAfterSaveAreWarnedAbout() {
        render(ScreenFixtures.family().copy(showDataControls = true,
            completed = CompletedSave(ScreenFixtures.nowMs, 1u), revision = 2u))
        compose.onNodeWithText("Last completed file save:", substring = true).performScrollTo()
        compose.onNodeWithText("Changes since that save are not in that file.").performScrollTo()
    }

    @Test fun unchangedLocalSaveDoesNotClaimMissingChanges() {
        render(ScreenFixtures.family().copy(showDataControls = true,
            completed = CompletedSave(ScreenFixtures.nowMs, 1u), revision = 1u))
        compose.onNodeWithText("Last completed file save:", substring = true).performScrollTo()
        compose.onNodeWithText("Changes since that save are not in that file.").assertDoesNotExist()
    }

    @Test fun sharedSaveShowsItsPointAndConservativeFreshness() {
        render(shared().copy(showDataControls = true,
            completed = CompletedSave(ScreenFixtures.nowMs, 0u)))
        compose.onNodeWithText("Last completed file save:", substring = true).performScrollTo()
        compose.onNodeWithText("This phone cannot confirm whether that file includes the latest shared history.",
            substring = true).performScrollTo()
    }
}
