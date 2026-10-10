package org.babytrack.app

import android.app.Application
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/** A release build shows Start sharing only after the preview-relay opt-in. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], application = Application::class)
class PreviewSharingOptInTest {
    @get:Rule val compose = createComposeRule()

    @Test
    fun releaseOptInRevealsStartSharing() {
        var optedIn by mutableStateOf(false)
        compose.setContent {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                FamilyScreen(
                    ScreenFixtures.family().copy(
                        showFamilySetup = true,
                        showPreviewSharingSwitch = true,
                        previewSharingOptIn = optedIn,
                        previewSharingEnabled = optedIn,
                    ),
                    FamilyActions(onTogglePreviewSharing = { optedIn = !optedIn }),
                )
            }
        }
        compose.onNodeWithText("Start sharing").assertDoesNotExist()
        val toggle = compose.onNodeWithText("Preview sharing", useUnmergedTree = false)
        toggle.performScrollTo().assertIsOff().performClick()
        compose.onNodeWithText("Preview sharing").assertIsOn()
        compose.onNodeWithText("Start sharing").performScrollTo()
        compose.onNodeWithText("Preview sharing").performScrollTo().performClick()
        compose.onNodeWithText("Start sharing").assertDoesNotExist()
    }
}
