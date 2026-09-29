package org.babytrack.app

import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.net.ConnectivityManager
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.babytrack_core_ffi.BindingException
import uniffi.babytrack_core_ffi.SharedSnapshotRow
import uniffi.babytrack_core_ffi.ActivityWhen
import uniffi.babytrack_core_ffi.FamilyRef
import uniffi.babytrack_core_ffi.NativeLocalStore

/** Opt-in hosted preview check; only the emulator runner supplies disposable data. */
@RunWith(AndroidJUnit4::class)
class PreviewTwoPhoneTest {
    @get:Rule val composeRule = createEmptyComposeRule()
    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext
    private val db get() = context.filesDir.resolve("families.db")
    private val familyFile get() = context.filesDir.resolve("preview-test-family.txt")

    @Before fun requireDisposablePreviewOptIn() {
        assumeTrue(InstrumentationRegistry.getArguments().getString("disposablePreview") == "true")
    }

    private fun managerFamily(): FamilyRef {
        val parts = familyFile.readText().split(':')
        return FamilyRef(parts[0].decodeHex(), parts[1].decodeHex())
    }

    private fun recipientFamily(): FamilyRef = ShareCoordinator(context, db.absolutePath).use {
        it.recipientFamilies().single()
    }

    private fun openFamilyTab() {
        val label = context.getString(R.string.nav_family)
        val tab = hasText(label) and SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Tab)
        composeRule.waitUntil(30_000) {
            composeRule.onAllNodesWithText(label).fetchSemanticsNodes().isNotEmpty()
        }
        composeRule.onNode(tab)
            .performSemanticsAction(SemanticsActions.OnClick)
    }

    @Test fun managerSharesAndCopiesInviteFromUi() {
        val family = NativeLocalStore.open(db.absolutePath).use { local ->
            local.createFamily(System.currentTimeMillis()).also {
                local.addChild(it, "Preview shared child", System.currentTimeMillis())
            }
        }
        familyFile.writeText("${family.familyId.hex()}:${family.deviceId.hex()}")
        context.getSharedPreferences("tracker_selection", Context.MODE_PRIVATE)
            .edit().putString("family", family.familyId.hex()).commit()
        ActivityScenario.launch(MainActivity::class.java).use {
            openFamilyTab()
            val share = context.getString(R.string.share_family_action)
            composeRule.waitUntil(30_000) {
                composeRule.onAllNodesWithText(share).fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(share).performScrollTo().performClick()
            composeRule.waitUntil(45_000) {
                ShareCoordinator(context, db.absolutePath).use { sharing -> sharing.isShared(family) }
            }
            assertEquals(PreviewRelay.origin, context.getSharedPreferences(
                "shared_relay_origins", Context.MODE_PRIVATE,
            ).getString(family.familyId.hex(), null))
            val invite = context.getString(R.string.create_invite)
            composeRule.waitUntil(30_000) {
                composeRule.onAllNodesWithText(invite).fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(invite).performScrollTo().performClick()
            val copy = context.getString(R.string.copy_android_invitation)
            composeRule.waitUntil(45_000) {
                composeRule.onAllNodesWithText(copy).fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(copy).performScrollTo().performClick()
            val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
            val link = clipboard.primaryClip?.getItemAt(0)?.coerceToText(context)?.toString()
                ?: error("Android invite was not copied")
            assertTrue(link.matches(Regex("babytrack://join#bt-invite=v1\\.[A-Za-z0-9_-]+")))
            context.filesDir.resolve("preview-test-invite.txt").writeText(link)
        }
    }

    @Test fun recipientStartsJoinFromLinkUi() {
        val link = InstrumentationRegistry.getArguments().getString("inviteLink")
            ?: error("inviteLink required")
        val intent = Intent(Intent.ACTION_VIEW, Uri.parse(link)).setClass(context, MainActivity::class.java)
        ActivityScenario.launch<MainActivity>(intent).use {
            val join = context.getString(R.string.join_or_retry)
            composeRule.waitUntil(30_000) {
                composeRule.onAllNodesWithText(join).fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(join).performScrollTo().performClick()
            composeRule.waitUntil(45_000) {
                ShareCoordinator(context, db.absolutePath).use { sharing ->
                    sharing.recipientFamilies().size == 1
                }
            }
        }
    }

    @Test fun managerAdvanceJoin() {
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            assertTrue(sharing.advanceManager(managerFamily(), PreviewRelay.origin).ready)
        }
    }

    @Test fun recipientAdvanceJoin() {
        val family = recipientFamily()
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            sharing.advanceRecipient(family)
        }
    }

    @Test fun recipientReadyAndReadsManagerNote() {
        val family = recipientFamily()
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            assertTrue(sharing.syncRecipientAndUpload(family).ready)
            val snapshot = sharing.snapshot(family)
            assertTrue(snapshot.children.any { it.name == "Preview shared child" })
            assertTrue(snapshot.activities.any { it.note == "Manager preview note" })
        }
    }

    @Test fun managerWritesNote() {
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            val family = managerFamily()
            val child = sharing.snapshot(family).children.single().id
            val now = System.currentTimeMillis()
            sharing.logNote(family, child, "Manager preview note", ActivityWhen(now, 0, now))
            assertTrue(sharing.syncAndUpload(family, PreviewRelay.origin).ready)
        }
    }

    @Test fun recipientWritesNote() {
        val family = recipientFamily()
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            val child = sharing.snapshot(family).children.single().id
            val now = System.currentTimeMillis()
            sharing.logNote(family, child, "Recipient preview note", ActivityWhen(now, 0, now))
            assertTrue(sharing.syncRecipientAndUpload(family).ready)
        }
    }

    @Test fun managerReadsRecipientNote() {
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            val family = managerFamily()
            assertTrue(sharing.syncAndUpload(family, PreviewRelay.origin).ready)
            assertTrue(sharing.snapshot(family).activities.any { it.note == "Recipient preview note" })
        }
    }

    private fun observeSnapshot(family: FamilyRef): SharedSnapshotRow? = try {
        ShareCoordinator(context, db.absolutePath).use { it.snapshot(family) }
    } catch (failure: BindingException.Rejected) {
        // This observer uses a separate connection from the activity. Retry only
        // its transient SQLite contention; all other core failures fail the test.
        if (failure.v1.contains("code: DatabaseBusy")) null else throw failure
    }

    private fun logWetDiaperFromUi(family: FamilyRef) {
        val before = ShareCoordinator(context, db.absolutePath).use { sharing ->
            sharing.snapshot(family).activities.count { it.kind == "diaper" }
        }
        ActivityScenario.launch(MainActivity::class.java).use {
            val today = hasText(context.getString(R.string.nav_today)) and
                SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Tab)
            composeRule.waitUntil(30_000) {
                composeRule.onAllNodesWithText(context.getString(R.string.nav_today))
                    .fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNode(today).performSemanticsAction(SemanticsActions.OnClick)
            val quick = context.getString(R.string.quick_wet_diaper)
            composeRule.waitUntil(30_000) {
                composeRule.onAllNodesWithText(quick).fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(quick).performScrollTo().performClick()
            composeRule.waitUntil(30_000) {
                observeSnapshot(family)?.activities?.count { it.kind == "diaper" } == before + 1
            }
        }
    }

    @Test fun managerLogsDiaperFromUi() = logWetDiaperFromUi(managerFamily())

    @Test fun recipientLogsDiaperFromUi() = logWetDiaperFromUi(recipientFamily())

    @Test fun managerSyncsAndReadsBothDiapers() {
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            val family = managerFamily()
            assertTrue(sharing.syncAndUpload(family, PreviewRelay.origin).ready)
            assertEquals(2, sharing.snapshot(family).activities.count { it.kind == "diaper" })
        }
    }

    @Test fun recipientSyncsAndReadsManagerDiaper() {
        val family = recipientFamily()
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            assertTrue(sharing.syncRecipientAndUpload(family).ready)
            assertEquals(1, sharing.snapshot(family).activities.count { it.kind == "diaper" })
        }
    }
    private fun expectedDiapers() = InstrumentationRegistry.getArguments()
        .getString("expectedDiapers")?.toInt() ?: error("expectedDiapers required")

    private fun checkSavedDiapers(family: FamilyRef) {
        ShareCoordinator(context, db.absolutePath).use { sharing ->
            val snapshot = sharing.snapshot(family)
            assertEquals(expectedDiapers(), snapshot.activities.count { it.kind == "diaper" })
            assertTrue(snapshot.unsentCount > 0uL)
        }
    }

    @Test fun managerChecksOfflineDiapersAfterRestart() = checkSavedDiapers(managerFamily())
    @Test fun recipientChecksOfflineDiapersAfterRestart() = checkSavedDiapers(recipientFamily())

    private fun logOfflineDiaper(family: FamilyRef) {
        val network = context.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        assertEquals("Offline logging must have no active network", null, network.activeNetwork)
        logWetDiaperFromUi(family)
        checkSavedDiapers(family)
    }

    @Test fun managerLogsOfflineDiaperFromUi() = logOfflineDiaper(managerFamily())
    @Test fun recipientLogsOfflineDiaperFromUi() = logOfflineDiaper(recipientFamily())

    private fun awaitForegroundConvergence(family: FamilyRef) {
        ActivityScenario.launch(MainActivity::class.java).use {
            // Read saved projection only: the activity must perform all relay work.
            composeRule.waitUntil(120_000) {
                val snapshot = observeSnapshot(family)
                snapshot != null &&
                    snapshot.activities.count { it.kind == "diaper" } == expectedDiapers() &&
                    snapshot.unsentCount == 0uL
            }
        }
    }

    @Test fun managerConvergesInForeground() = awaitForegroundConvergence(managerFamily())
    @Test fun recipientConvergesInForeground() = awaitForegroundConvergence(recipientFamily())

}

private fun ByteArray.hex(): String = joinToString("") { "%02x".format(it.toInt() and 255) }
private fun String.decodeHex(): ByteArray = chunked(2).map { it.toInt(16).toByte() }.toByteArray()
