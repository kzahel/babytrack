package org.babytrack.app

import android.view.accessibility.AccessibilityNodeInfo
import android.content.Intent
import android.Manifest
import android.app.NotificationManager
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import android.app.job.JobScheduler
import android.os.ParcelFileDescriptor
import android.widget.FrameLayout
import android.widget.TextView
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.babytrack_core_ffi.NativeLocalStore
import uniffi.babytrack_core_ffi.NativeSharedStore
import uniffi.babytrack_core_ffi.ActivityWhen
import uniffi.babytrack_core_ffi.previewInvitation

@RunWith(AndroidJUnit4::class)
class SharingRelayTest {
    @get:Rule val composeRule = createEmptyComposeRule()

    @Test
    fun localTimerNotificationTracksSavedStartAndStop() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        instrumentation.uiAutomation.grantRuntimePermission(
            context.packageName, Manifest.permission.POST_NOTIFICATIONS,
        )
        val path = context.filesDir.resolve("timer-notification-${System.nanoTime()}.db")
        val manager = context.getSystemService(NotificationManager::class.java)
        NativeLocalStore.open(path.absolutePath).use { local ->
            ShareCoordinator(context, path.absolutePath).use { sharing ->
                val now = System.currentTimeMillis()
                val family = local.createFamily(now)
                val child = local.addChild(family, "Timer child", now)
                val activity = local.startSleep(family, child, ActivityWhen(now, 0, now))
                val running = runningSleepCount(local, sharing, listOf(family), emptyList())
                assertEquals(1, running)
                assertEquals(
                    context.getString(R.string.sleep_widget_one),
                    SleepTimerWidget.views(context, running).apply(context, FrameLayout(context))
                        .findViewById<TextView>(R.id.widget_status).text.toString(),
                )
                SleepTimerNotifications.update(context, running)
                assertTrue("Running timer notification should appear", waitForSleepNotification(manager, context, true))
                SleepTimerNotifications.update(context, 0)
                assertTrue("Cleared notification should disappear", waitForSleepNotification(manager, context, false))
                refreshSleepTimers(context, path.absolutePath)
                assertTrue("Saved timer should restore its notification", waitForSleepNotification(manager, context, true))
                local.stopSleep(family, child, activity, now + 60_000, 0, now + 60_000)
                val stopped = runningSleepCount(local, sharing, listOf(family), emptyList())
                assertEquals(0, stopped)
                assertEquals(
                    context.getString(R.string.sleep_widget_none),
                    SleepTimerWidget.views(context, stopped).apply(context, FrameLayout(context))
                        .findViewById<TextView>(R.id.widget_status).text.toString(),
                )
                SleepTimerNotifications.update(context, stopped)
                assertTrue("Stopped timer notification should clear", waitForSleepNotification(manager, context, false))
            }
        }
    }

    private fun waitForSleepNotification(
        manager: NotificationManager,
        context: android.content.Context,
        expected: Boolean,
    ): Boolean {
        val deadline = System.currentTimeMillis() + 10_000
        do {
            val present = manager.activeNotifications.any {
                it.notification.extras.getString("android.title") == context.getString(R.string.sleep_notification_title)
            }
            if (present == expected) return true
            Thread.sleep(50)
        } while (System.currentTimeMillis() < deadline)
        return false
    }

    @Test
    fun claimedLinkIsTerminalButSavedCompetingClaimStaysUnknown() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("claimed-manager-${System.nanoTime()}.db")
        val firstDb = context.filesDir.resolve("claimed-first-${System.nanoTime()}.db")
        val secondDb = context.filesDir.resolve("claimed-second-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(managerDb.absolutePath).use {
            it.createFamily(System.currentTimeMillis())
        }
        val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
        }
        val preview = previewInvitation(fragment)
        val wrapping = DeviceWrappingKey(context).loadOrCreate()
        val waiting = try {
            NativeSharedStore.open(secondDb.absolutePath).use { core ->
                core.prepareJoin(fragment, RelayTransport(origin).get(preview.controlPath, preview.readAuth), wrapping).family
            }
        } finally {
            wrapping.fill(0)
        }
        ShareCoordinator(context, firstDb.absolutePath).use { sharing ->
            sharing.claim(fragment)
        }
        ShareCoordinator(context, context.filesDir.resolve("claimed-link-${System.nanoTime()}.db").absolutePath).use { sharing ->
            val result = runCatching { sharing.claim(fragment) }
            assertEquals(InvitationTerminalReason.CLAIMED, (result.exceptionOrNull() as? InvitationTerminal)?.reason)
        }
        ShareCoordinator(context, secondDb.absolutePath).use { sharing ->
            val result = runCatching { sharing.advanceRecipient(waiting) }
            assertTrue(result.isFailure)
            assertTrue(result.exceptionOrNull() !is InvitationTerminal)
            assertTrue(sharing.recipientFamilies().any { it.familyId.contentEquals(waiting.familyId) })
        }
        ShareCoordinator(context, secondDb.absolutePath).use { sharing ->
            val result = runCatching { sharing.advanceRecipient(waiting) }
            assertTrue(result.isFailure)
            assertTrue(result.exceptionOrNull() !is InvitationTerminal)
        }
    }

    @Test
    fun committedRetryWithLostResponseRemainsRetryableAfterRestart() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("lost-retry-manager-${System.nanoTime()}.db")
        val recipientDb = context.filesDir.resolve("lost-retry-recipient-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(managerDb.absolutePath).use { local ->
            val created = local.createFamily(System.currentTimeMillis())
            local.addChild(created, "Recovered join child", System.currentTimeMillis())
            created
        }
        val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
        }
        val preview = previewInvitation(fragment)
        val wrapping = DeviceWrappingKey(context).loadOrCreate()
        val recipient = try {
            NativeSharedStore.open(recipientDb.absolutePath).use { core ->
                core.prepareJoin(fragment, RelayTransport(origin).get(preview.controlPath, preview.readAuth), wrapping).family
            }
        } finally {
            wrapping.fill(0)
        }
        var dropOnce = true
        val relayProvider: (String) -> RelayTransport = { relayOrigin ->
            object : RelayTransport(relayOrigin) {
                override fun post(path: String, bytes: ByteArray, allowBatchConflict: Boolean): ByteArray {
                    val accepted = super.post(path, bytes, allowBatchConflict)
                    if (dropOnce && path.endsWith("/control")) {
                        dropOnce = false
                        throw IllegalStateException("simulated lost accepted claim response")
                    }
                    return accepted
                }
            }
        }
        ShareCoordinator(context, recipientDb.absolutePath, relayProvider).use { sharing ->
            val failure = runCatching { sharing.advanceRecipient(recipient) }.exceptionOrNull()
            assertEquals("simulated lost accepted claim response", failure?.message)
        }
        val afterLostResponse = DeviceWrappingKey(context).loadOrCreate()
        try {
            NativeSharedStore.open(recipientDb.absolutePath).use { core ->
                assertEquals(null, core.savedJoinTerminalStatus(recipient, afterLostResponse))
                assertEquals(2u.toUByte(), core.recipientFirstJoinAction(recipient, afterLostResponse))
            }
        } finally {
            afterLostResponse.fill(0)
        }
        ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceRecipient(recipient).awaitingGrant)
        }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceManager(family, origin).ready)
        }
        ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceRecipient(recipient).awaitingGrant)
        }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceManager(family, origin).ready)
        }
        ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceRecipient(recipient).ready)
            assertEquals("Recovered join child", sharing.snapshot(recipient).children.single().name)
        }
    }

    @Test
    fun managerStopsPendingJoinAfterLostRemovalResponse() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("pending-remove-manager-${System.nanoTime()}.db")
        val recipientDb = context.filesDir.resolve("pending-remove-recipient-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(managerDb.absolutePath).use { local ->
            val created = local.createFamily(System.currentTimeMillis())
            local.addChild(created, "Still with manager", System.currentTimeMillis())
            created
        }
        val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
        }
        val recipient = ShareCoordinator(context, recipientDb.absolutePath).use { it.claim(fragment).family }
        val pending = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            assertTrue(sharing.syncAndUpload(family, origin).ready)
            sharing.snapshot(family).pendingDevices.single()
        }
        assertArrayEquals(recipient.deviceId, pending.deviceId)
        val wrapping = DeviceWrappingKey(context).loadOrCreate()
        try {
            val prepared = NativeSharedStore.open(managerDb.absolutePath).use { core ->
                core.preparePendingRemoval(family, wrapping, pending.invitationId, pending.deviceId)
            }
            RelayTransport(origin).post(
                "/v1/families/${family.familyId.joinToString("") { "%02x".format(it.toInt() and 255) }}/control",
                prepared.candidateBytes,
            ) // The committed response is lost before local confirmation.
        } finally {
            wrapping.fill(0)
        }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            val after = sharing.removePendingDevice(family, origin, pending.invitationId, pending.deviceId)
            assertTrue(after.pendingDevices.isEmpty())
            assertEquals(1, after.devices.size)
            assertEquals("Still with manager", after.children.single().name)
            assertTrue(sharing.syncAndUpload(family, origin).ready)
        }
        ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            val stopped = sharing.advanceRecipient(recipient)
            assertEquals(8u.toUByte(), stopped.joinPhase)
            assertTrue(!stopped.awaitingGrant && !stopped.ready)
            assertTrue(runCatching { sharing.snapshot(recipient) }.isFailure)
        }
        ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            assertEquals(8u.toUByte(), sharing.advanceRecipient(recipient).joinPhase)
        }
    }

    @Test
    fun managerStopsPendingDeviceFromAccessUi() {
        wakeEmulatorScreen()
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("families.db")
        val recipientDb = context.filesDir.resolve("pending-ui-recipient-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(managerDb.absolutePath).use { it.createFamily(System.currentTimeMillis()) }
        val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
        }
        val recipient = ShareCoordinator(context, recipientDb.absolutePath).use { it.claim(fragment).family }
        val pending = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            assertTrue(sharing.syncAndUpload(family, origin).ready)
            sharing.snapshot(family).pendingDevices.single()
        }
        val familyHex = family.familyId.joinToString("") { "%02x".format(it.toInt() and 255) }
        context.getSharedPreferences("shared_relay_origins", android.content.Context.MODE_PRIVATE)
            .edit().putString(familyHex, origin).commit()
        context.getSharedPreferences("tracker_selection", android.content.Context.MODE_PRIVATE)
            .edit().putString("family", familyHex).commit()
        ActivityScenario.launch(MainActivity::class.java).use {
            val shortId = pending.deviceId.joinToString("") { "%02x".format(it.toInt() and 255) }.take(8)
            val label = context.getString(R.string.device_short_id, shortId)
            val button = context.getString(R.string.remove_pending_device, label)
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText(button).fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(button).performScrollTo().performClick()
            val confirm = context.getString(R.string.confirm_remove_pending_device)
            composeRule.waitUntil(15_000) {
                composeRule.onAllNodesWithText(confirm).fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(confirm).performClick()
            val deadline = System.currentTimeMillis() + 25_000
            var removed = false
            while (System.currentTimeMillis() < deadline) {
                removed = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
                    sharing.snapshot(family).pendingDevices.isEmpty()
                }
                if (removed) break
                Thread.sleep(200)
            }
            assertTrue("The UI action should commit pending-device removal", removed)
        }
        ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            assertEquals(8u.toUByte(), sharing.advanceRecipient(recipient).joinPhase)
        }
    }

    @Test
    fun removedRecipientCanContinueInPrivateCopyFromTheUi() {
        wakeEmulatorScreen()
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("copy-ui-manager-${System.nanoTime()}.db")
        val recipientDb = context.filesDir.resolve("families.db")
        val manager = NativeLocalStore.open(managerDb.absolutePath).use { local ->
            val family = local.createFamily(System.currentTimeMillis())
            local.addChild(family, "UI private child", System.currentTimeMillis())
            family
        }
        val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(manager, origin, publicKey)
            sharing.invite(manager, origin, 1u.toUByte())
        }
        val recipient = ShareCoordinator(context, recipientDb.absolutePath).use { it.claim(fragment).family }
        ShareCoordinator(context, managerDb.absolutePath).use { assertTrue(it.advanceManager(manager, origin).ready) }
        ShareCoordinator(context, recipientDb.absolutePath).use { assertTrue(it.advanceRecipient(recipient).awaitingGrant) }
        ShareCoordinator(context, managerDb.absolutePath).use { assertTrue(it.advanceManager(manager, origin).ready) }
        ShareCoordinator(context, recipientDb.absolutePath).use { assertTrue(it.advanceRecipient(recipient).ready) }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.removeDevice(manager, origin, recipient.deviceId)
        }
        ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            val removed = sharing.advanceRecipient(recipient)
            assertTrue(removed.removed)
            assertEquals(null, removed.privateCopy)
        }
        val before = NativeLocalStore.open(recipientDb.absolutePath).use { local ->
            local.families().map { it.familyId.joinToString("") { byte -> "%02x".format(byte.toInt() and 255) } }.toSet()
        }
        ActivityScenario.launch(MainActivity::class.java).use {
            val button = context.getString(R.string.continue_in_private_copy)
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText(button).fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(button).performScrollTo().performClick()
            val deadline = System.currentTimeMillis() + 25_000
            var copied = false
            while (System.currentTimeMillis() < deadline) {
                copied = NativeLocalStore.open(recipientDb.absolutePath).use { local ->
                    local.families().any { family ->
                        val id = family.familyId.joinToString("") { byte -> "%02x".format(byte.toInt() and 255) }
                        id !in before && local.children(family).any { child -> child.name == "UI private child" }
                    }
                }
                if (copied) break
                Thread.sleep(200)
            }
            assertTrue("The recovery action should create a local Family with held history", copied)
        }
    }

    @Test
    fun managerCancelsUnusedInvitationBeforeClaim() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("cancel-manager-${System.nanoTime()}.db")
        val recipientDb = context.filesDir.resolve("cancel-recipient-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(managerDb.absolutePath).use {
            it.createFamily(System.currentTimeMillis())
        }
        val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
        }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            val invitationId = sharing.unusedInvitationIds(family).single()
            val key = DeviceWrappingKey(context).loadOrCreate()
            try {
                val first = NativeSharedStore.open(managerDb.absolutePath).use { core ->
                    core.prepareInviteCancel(family, key, invitationId).candidateBytes
                }
                val resumed = NativeSharedStore.open(managerDb.absolutePath).use { core ->
                    core.prepareInviteCancel(family, key, invitationId).candidateBytes
                }
                assertArrayEquals(first, resumed)
            } finally {
                key.fill(0)
            }
            sharing.cancelInvitation(family, origin, invitationId)
            assertTrue(sharing.unusedInvitationIds(family).isEmpty())
        }
        ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            val result = runCatching { sharing.claim(fragment) }
            val terminal = result.exceptionOrNull() as? InvitationTerminal
            assertEquals(InvitationTerminalReason.CANCELED, terminal?.reason)
            assertTrue(sharing.recipientFamilies().isEmpty())
        }
    }

    @Test
    fun lostCancelResponseDoesNotBlockAnotherInvitation() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("lost-cancel-manager-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(managerDb.absolutePath).use {
            it.createFamily(System.currentTimeMillis())
        }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
            sharing.invite(family, origin, 1u.toUByte())
            val ids = sharing.unusedInvitationIds(family)
            assertEquals(2, ids.size)
            val key = DeviceWrappingKey(context).loadOrCreate()
            val first = try {
                NativeSharedStore.open(managerDb.absolutePath).use { core ->
                    core.prepareInviteCancel(family, key, ids[0]).candidateBytes
                }
            } finally {
                key.fill(0)
            }
            RelayTransport(origin).post(
                "/v1/families/${family.familyId.joinToString("") { "%02x".format(it) }}/control",
                first,
            ) // The signed response is lost before local confirmation.
            sharing.cancelInvitation(family, origin, ids[1])
            assertTrue(sharing.unusedInvitationIds(family).isEmpty())
        }
    }

    @Test
    fun managerCanCancelAnUnusedInvitationFromTheUi() {
        wakeEmulatorScreen()
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val db = context.filesDir.resolve("families.db")
        val family = NativeLocalStore.open(db.absolutePath).use {
            it.createFamily(System.currentTimeMillis())
        }
        val invitationId = ShareCoordinator(context, db.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
            sharing.unusedInvitationIds(family).single()
        }
        context.getSharedPreferences("shared_relay_origins", android.content.Context.MODE_PRIVATE)
            .edit().putString(family.familyId.joinToString("") { "%02x".format(it) }, origin).commit()
        context.getSharedPreferences("tracker_selection", android.content.Context.MODE_PRIVATE)
            .edit().putString("family", family.familyId.joinToString("") { "%02x".format(it) }).commit()
        val shortId = invitationId.joinToString("") { "%02x".format(it) }.take(8)
        ActivityScenario.launch(MainActivity::class.java).use {
            val button = context.getString(R.string.cancel_invitation, shortId)
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText(button).fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(button).performScrollTo().performClick()
            val confirm = context.getString(R.string.confirm_cancel_invitation)
            composeRule.waitUntil(15_000) {
                composeRule.onAllNodesWithText(confirm).fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(confirm).performClick()
            val committedDeadline = System.currentTimeMillis() + 25_000
            var canceled = false
            while (System.currentTimeMillis() < committedDeadline) {
                canceled = ShareCoordinator(context, db.absolutePath).use { sharing ->
                    sharing.unusedInvitationIds(family).isEmpty()
                }
                if (canceled) break
                Thread.sleep(200)
            }
            assertTrue("The UI action should commit the invitation cancellation", canceled)
        }
    }
    @Test
    fun sharedInvitationOpensJoinFormWithoutRedeemingIt() {
        wakeEmulatorScreen()
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val fragment = "#bt-invite=v1.received-for-review"
        val previousRecipients = ShareCoordinator(
            context,
            context.filesDir.resolve("families.db").absolutePath,
        ).use { it.recipientFamilies().size }
        val intent = Intent(context, MainActivity::class.java).apply {
            action = Intent.ACTION_SEND
            type = "text/plain"
            putExtra(Intent.EXTRA_TEXT, fragment)
        }
        ActivityScenario.launch<MainActivity>(intent).use {
            val deadline = System.currentTimeMillis() + 15_000
            var visible = false
            while (System.currentTimeMillis() < deadline) {
                val root = instrumentation.uiAutomation.rootInActiveWindow
                visible = root?.containsText(fragment) == true
                if (visible) break
                root?.scrollForward()
                Thread.sleep(250)
            }
            assertTrue("Shared invitation should prefill the join form", visible)
            ShareCoordinator(context, context.filesDir.resolve("families.db").absolutePath).use { sharing ->
                assertEquals("Receiving an invitation must not claim it", previousRecipients, sharing.recipientFamilies().size)
            }
        }
        val link = Intent(Intent.ACTION_VIEW, android.net.Uri.parse("babytrack://join$fragment")).apply {
            addCategory(Intent.CATEGORY_BROWSABLE)
            setPackage(context.packageName)
        }
        assertEquals(MainActivity::class.java.name,
            link.resolveActivity(context.packageManager)?.className)
        ActivityScenario.launch<MainActivity>(link).use {
            val deadline = System.currentTimeMillis() + 15_000
            var visible = false
            while (System.currentTimeMillis() < deadline) {
                val root = instrumentation.uiAutomation.rootInActiveWindow
                visible = root?.containsText(fragment) == true
                if (visible) break
                root?.scrollForward()
                Thread.sleep(250)
            }
            assertTrue("Invitation link should prefill the join form", visible)
            ShareCoordinator(context, context.filesDir.resolve("families.db").absolutePath).use { sharing ->
                assertEquals("Opening an invitation link must not claim it",
                    previousRecipients, sharing.recipientFamilies().size)
            }
        }
    }

    @Test
    fun invitationLinkStartsOneActionJoinThroughTheUi() {
        wakeEmulatorScreen()
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("ui-join-manager-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(managerDb.absolutePath).use { local ->
            local.createFamily(System.currentTimeMillis())
        }
        val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
        }
        val link = Intent(Intent.ACTION_VIEW, android.net.Uri.parse("babytrack://join$fragment")).apply {
            addCategory(Intent.CATEGORY_BROWSABLE)
            setPackage(context.packageName)
        }
        ActivityScenario.launch<MainActivity>(link).use { scenario ->
            val label = context.getString(R.string.join_or_retry)
            composeRule.waitUntil(25_000) {
                runCatching { composeRule.onNodeWithText(label).assertIsDisplayed() }.isSuccess
            }
            composeRule.onNodeWithText(label).assertIsDisplayed().performClick()

            val saved = context.filesDir.resolve("families.db").absolutePath
            val claimDeadline = System.currentTimeMillis() + 45_000
            var committed = false
            var joiningIndex = -1
            while (System.currentTimeMillis() < claimDeadline) {
                val key = DeviceWrappingKey(context).loadOrCreate()
                committed = try {
                    NativeSharedStore.open(saved).use { core ->
                        val recipients = core.recipientFamilies().filterNot { core.isRemoved(it) }
                        joiningIndex = recipients.indexOfFirst { it.familyId.contentEquals(family.familyId) }
                        recipients.getOrNull(joiningIndex)
                            ?.let { core.recipientFirstJoinAction(it, key) == 0u.toUByte() } ?: false
                    }
                } catch (failure: Exception) {
                    // The UI coordinator can hold the SQLite writer while this
                    // independent assertion connection reads its claim state.
                    // Retry only that transient lock; other failures are real.
                    if (!failure.message.orEmpty().contains("DatabaseBusy")) throw failure
                    false
                } finally {
                    key.fill(0)
                }
                if (committed) break
                Thread.sleep(200)
            }
            assertTrue("The single UI action should commit a saved recipient claim", committed)
            val joining = context.getString(R.string.joining_family_number, joiningIndex + 1)
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText(joining).fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(joining).performScrollTo().assertIsDisplayed().performClick()
            scenario.recreate()
            composeRule.waitUntil(25_000) {
                composeRule.onAllNodesWithText(joining).fetchSemanticsNodes().isNotEmpty()
            }
            composeRule.onNodeWithText(joining).performScrollTo().assertIsDisplayed().performClick()
            composeRule.onNodeWithText(context.getString(R.string.saved_join_pending))
                .performScrollTo().assertIsDisplayed()
            assertEquals(0, composeRule.onAllNodesWithText(fragment).fetchSemanticsNodes().size)
        }
    }

    @Test
    fun activityRecreationResumesSavedRecipientClaim() {
        wakeEmulatorScreen()
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("activity-manager-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(managerDb.absolutePath).use { it.createFamily(System.currentTimeMillis()) }
        val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
        }
        val preview = previewInvitation(fragment)
        val wrapping = DeviceWrappingKey(context).loadOrCreate()
        val recipient = try {
            NativeSharedStore.open(context.filesDir.resolve("families.db").absolutePath).use { core ->
                core.prepareJoin(fragment, RelayTransport(origin).get(preview.controlPath, preview.readAuth), wrapping).family
            }
        } finally {
            wrapping.fill(0)
        }
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            scenario.recreate()
            val deadline = System.currentTimeMillis() + 12_000
            var resumed = false
            var visible = false
            while (System.currentTimeMillis() < deadline) {
                val root = instrumentation.uiAutomation.rootInActiveWindow
                visible = root?.packageName?.toString() == context.packageName
                val wrappingAfter = DeviceWrappingKey(context).loadOrCreate()
                resumed = try {
                    NativeSharedStore.open(context.filesDir.resolve("families.db").absolutePath).use { core ->
                        core.recipientFirstJoinAction(recipient, wrappingAfter) == 0u.toUByte()
                    }
                } finally {
                    wrappingAfter.fill(0)
                }
                if (visible && resumed) break
                Thread.sleep(100)
            }
            assertTrue("Recreated UI should resume and confirm the saved claim (visible=$visible, resumed=$resumed)", visible && resumed)
        }
        ShareCoordinator(context, context.filesDir.resolve("families.db").absolutePath).use { sharing ->
            assertTrue(sharing.recipientFamilies().any { it.familyId.contentEquals(recipient.familyId) })
        }
    }

    private fun AccessibilityNodeInfo.containsText(text: String): Boolean {
        if (this.text?.toString()?.contains(text) == true) return true
        return (0 until childCount).any { index -> getChild(index)?.containsText(text) == true }
    }

    private fun AccessibilityNodeInfo.scrollForward(): Boolean {
        if (isScrollable && performAction(AccessibilityNodeInfo.ACTION_SCROLL_FORWARD)) return true
        return (0 until childCount).any { index -> getChild(index)?.scrollForward() == true }
    }

    private fun wakeEmulatorScreen() {
        val automation = InstrumentationRegistry.getInstrumentation().uiAutomation
        for (command in listOf("input keyevent KEYCODE_WAKEUP", "wm dismiss-keyguard")) {
            ParcelFileDescriptor.AutoCloseInputStream(automation.executeShellCommand(command))
                .bufferedReader().use { it.readText() }
        }
    }

    @Test
    fun savedClaimRetriesByFamilyAfterPrecommitFailureOrLostResponse() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        for (committedBeforeRestart in listOf(false, true)) {
            val managerDb = context.filesDir.resolve("claim-manager-${System.nanoTime()}.db")
            val recipientDb = context.filesDir.resolve("claim-recipient-${System.nanoTime()}.db")
            val family = NativeLocalStore.open(managerDb.absolutePath).use { local ->
                local.createFamily(System.currentTimeMillis())
            }
            val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
                sharing.promote(family, origin, publicKey)
                sharing.invite(family, origin, 1u.toUByte())
            }
            val preview = previewInvitation(fragment)
            val relay = RelayTransport(origin)
            val wrapping = DeviceWrappingKey(context).loadOrCreate()
            val prepared = try {
                NativeSharedStore.open(recipientDb.absolutePath).use { core ->
                    val page = relay.get(preview.controlPath, preview.readAuth)
                    core.prepareJoin(fragment, page, wrapping)
                }
            } finally {
                wrapping.fill(0)
            }
            if (committedBeforeRestart) {
                relay.post("/v1/families/${family.familyId.joinToString("") { "%02x".format(it) }}/control", prepared.candidateBytes)
                // Drop the signed response before the local claim is confirmed.
            }
            ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
                assertArrayEquals(prepared.family.familyId, sharing.recipientFamilies().single().familyId)
                assertEquals(origin, sharing.recipientOrigin(prepared.family))
                assertTrue(sharing.advanceRecipient(prepared.family).awaitingGrant)
                assertArrayEquals(prepared.candidateBytes, sharing.retryClaim(prepared.family).candidateBytes)
            }
        }
    }

    @Test
    fun scheduledJobRetriesSavedRecipientClaim() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("job-claim-manager-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(managerDb.absolutePath).use { it.createFamily(System.currentTimeMillis()) }
        val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
        }
        val preview = previewInvitation(fragment)
        val wrapping = DeviceWrappingKey(context).loadOrCreate()
        val recipient = try {
            NativeSharedStore.open(context.filesDir.resolve("families.db").absolutePath).use { core ->
                core.prepareJoin(fragment, RelayTransport(origin).get(preview.controlPath, preview.readAuth), wrapping).family
            }
        } finally {
            wrapping.fill(0)
        }
        SharedSyncJobService.schedule(context)
        val before = context.getSharedPreferences("shared_background_sync", android.content.Context.MODE_PRIVATE)
            .getLong("last_attempt_ms", 0)
        val command = instrumentation.uiAutomation.executeShellCommand("cmd jobscheduler run -f org.babytrack.app 3400")
        ParcelFileDescriptor.AutoCloseInputStream(command).bufferedReader().use { it.readText() }
        val deadline = System.currentTimeMillis() + 15_000
        var resumed = false
        while (System.currentTimeMillis() < deadline) {
            val newAttempt = context.getSharedPreferences("shared_background_sync", android.content.Context.MODE_PRIVATE)
                .getLong("last_attempt_ms", 0) > before
            if (newAttempt) {
                val key = DeviceWrappingKey(context).loadOrCreate()
                resumed = try {
                    NativeSharedStore.open(context.filesDir.resolve("families.db").absolutePath).use { core ->
                        core.recipientFirstJoinAction(recipient, key) == 0u.toUByte()
                    }
                } finally {
                    key.fill(0)
                }
            }
            if (newAttempt && resumed) break
            Thread.sleep(100)
        }
        assertTrue("Background job should confirm the saved claim", resumed)
    }

    @Test
    fun scheduledJobUploadsSavedManagerEdit() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val database = context.filesDir.resolve("families.db")
        val family = NativeLocalStore.open(database.absolutePath).use { local ->
            local.createFamily(System.currentTimeMillis())
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.addChild(family, "Background child", System.currentTimeMillis())
            assertEquals(1uL, sharing.snapshot(family).unsentCount)
        }
        context.getSharedPreferences("shared_relay_origins", android.content.Context.MODE_PRIVATE)
            .edit().putString(family.familyId.joinToString("") { "%02x".format(it) }, origin).commit()
        SharedSyncJobService.schedule(context)
        assertTrue(context.getSystemService(JobScheduler::class.java).getPendingJob(3400) != null)
        val before = context.getSharedPreferences("shared_background_sync", android.content.Context.MODE_PRIVATE)
            .getLong("last_attempt_ms", 0)
        val command = instrumentation.uiAutomation.executeShellCommand("cmd jobscheduler run -f org.babytrack.app 3400")
        ParcelFileDescriptor.AutoCloseInputStream(command).bufferedReader().use { it.readText() }
        val deadline = System.currentTimeMillis() + 15_000
        while (System.currentTimeMillis() < deadline) {
            val done = context.getSharedPreferences("shared_background_sync", android.content.Context.MODE_PRIVATE)
                .getLong("last_attempt_ms", 0) > before
            if (done) break
            Thread.sleep(100)
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            assertEquals(0uL, sharing.snapshot(family).unsentCount)
        }
    }

    @Test
    fun asynchronousJoinAdvancesWithoutManualHolderApproval() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val managerDb = context.filesDir.resolve("auto-manager-${System.nanoTime()}.db")
        val recipientDb = context.filesDir.resolve("auto-recipient-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(managerDb.absolutePath).use { local ->
            val created = local.createFamily(System.currentTimeMillis())
            local.addChild(created, "Async child", System.currentTimeMillis())
            created
        }
        val fragment = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(family, origin, publicKey)
            sharing.invite(family, origin, 1u.toUByte())
        }
        val recipient = ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            sharing.claim(fragment).family
        }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceManager(family, origin).ready)
        }
        ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceRecipient(recipient).awaitingGrant)
        }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceManager(family, origin).ready)
        }
        ShareCoordinator(context, recipientDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceRecipient(recipient).ready)
            assertEquals("Async child", sharing.snapshot(recipient).children.single().name)
        }
    }

    @Test
    fun admittedManagerInvitesAndGrantsAnotherDevice() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val suffix = System.nanoTime()
        val managerDb = context.filesDir.resolve("handoff-manager-$suffix.db")
        val holderDb = context.filesDir.resolve("handoff-holder-$suffix.db")
        val thirdDb = context.filesDir.resolve("handoff-third-$suffix.db")
        val manager = NativeLocalStore.open(managerDb.absolutePath).use { local ->
            val family = local.createFamily(System.currentTimeMillis())
            local.addChild(family, "Handoff child", System.currentTimeMillis())
            family
        }
        val holderLink = ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            sharing.promote(manager, origin, publicKey)
            sharing.invite(manager, origin, 2u.toUByte())
        }
        val holder = ShareCoordinator(context, holderDb.absolutePath).use { it.claim(holderLink).family }
        ShareCoordinator(context, managerDb.absolutePath).use { assertTrue(it.advanceManager(manager, origin).ready) }
        ShareCoordinator(context, holderDb.absolutePath).use { assertTrue(it.advanceRecipient(holder).awaitingGrant) }
        ShareCoordinator(context, managerDb.absolutePath).use { assertTrue(it.advanceManager(manager, origin).ready) }
        val thirdLink = ShareCoordinator(context, holderDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceRecipient(holder).ready)
            assertTrue(sharing.isAdmittedManager(holder))
            sharing.invite(holder, origin, 1u.toUByte())
        }
        NativeLocalStore.open(thirdDb.absolutePath).use { it.createFamily(System.currentTimeMillis()) }
        val third = ShareCoordinator(context, thirdDb.absolutePath).use { it.claim(thirdLink).family }
        NativeLocalStore.open(thirdDb.absolutePath).use { local ->
            ShareCoordinator(context, thirdDb.absolutePath).use { sharing ->
                assertEquals(1, loadTrackerData(local, sharing, null, null, null).families.size)
            }
        }
        ShareCoordinator(context, holderDb.absolutePath).use { assertTrue(it.advanceRecipient(holder).ready) }
        ShareCoordinator(context, thirdDb.absolutePath).use { assertTrue(it.advanceRecipient(third).awaitingGrant) }
        ShareCoordinator(context, holderDb.absolutePath).use { assertTrue(it.advanceRecipient(holder).ready) }
        ShareCoordinator(context, thirdDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceRecipient(third).ready)
            assertEquals("Handoff child", sharing.snapshot(third).children.single().name)
            NativeLocalStore.open(thirdDb.absolutePath).use { local ->
                val selected = third.familyId.joinToString("") { "%02x".format(it) }
                val tracker = loadTrackerData(local, sharing, selected, null, null)
                assertEquals(2, tracker.families.size)
                assertEquals(selected, tracker.activeFamilyKey)
                assertEquals("Handoff child", tracker.familyChildNames[selected])
                assertTrue(tracker.shared)
                assertTrue(!tracker.activeFamilyIsLocal)
                assertEquals("Handoff child", tracker.children.single().name)
            }
        }
        ShareCoordinator(context, holderDb.absolutePath).use { sharing ->
            sharing.addChild(holder, "Holder child", System.currentTimeMillis())
            assertTrue(sharing.syncRecipientAndUpload(holder).ready)
        }
        ShareCoordinator(context, thirdDb.absolutePath).use { sharing ->
            assertTrue(sharing.syncRecipientAndUpload(third).ready)
            assertTrue(sharing.snapshot(third).children.any { it.name == "Holder child" })
        }
        val fourthDb = context.filesDir.resolve("handoff-fourth-$suffix.db")
        val fourthLink = ShareCoordinator(context, holderDb.absolutePath).use { sharing ->
            sharing.invite(holder, origin, 1u.toUByte())
        }
        val fourth = ShareCoordinator(context, fourthDb.absolutePath).use { it.claim(fourthLink).family }
        ShareCoordinator(context, holderDb.absolutePath).use { sharing ->
            assertTrue(sharing.syncRecipientAndUpload(holder).ready)
            val pendingFourth = sharing.snapshot(holder).pendingDevices.single {
                it.deviceId.contentEquals(fourth.deviceId)
            }
            val after = sharing.removePendingDevice(
                holder, origin, pendingFourth.invitationId, fourth.deviceId,
            )
            assertTrue(after.pendingDevices.none { it.deviceId.contentEquals(fourth.deviceId) })
        }
        ShareCoordinator(context, fourthDb.absolutePath).use { sharing ->
            assertEquals(8u.toUByte(), sharing.advanceRecipient(fourth).joinPhase)
        }
        val unusedLink = ShareCoordinator(context, holderDb.absolutePath).use { sharing ->
            val link = sharing.invite(holder, origin, 1u.toUByte())
            sharing.cancelInvitation(holder, origin, sharing.unusedInvitationIds(holder).single())
            link
        }
        val canceledDb = context.filesDir.resolve("handoff-canceled-$suffix.db")
        ShareCoordinator(context, canceledDb.absolutePath).use { sharing ->
            assertEquals(InvitationTerminalReason.CANCELED,
                (runCatching { sharing.claim(unusedLink) }.exceptionOrNull() as? InvitationTerminal)?.reason)
        }
        ShareCoordinator(context, holderDb.absolutePath).use { sharing ->
            val promoted = sharing.changeDeviceRole(holder, origin, third.deviceId, 2u.toUByte())
            assertEquals(2u.toUByte(), promoted.devices.single {
                it.deviceId.contentEquals(third.deviceId)
            }.role)
        }
        ShareCoordinator(context, thirdDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceRecipient(third).ready)
            assertTrue(sharing.isAdmittedManager(third))
        }
        ShareCoordinator(context, holderDb.absolutePath).use { sharing ->
            val demoted = sharing.changeDeviceRole(holder, origin, third.deviceId, 1u.toUByte())
            assertEquals(1u.toUByte(), demoted.devices.single {
                it.deviceId.contentEquals(third.deviceId)
            }.role)
        }
        ShareCoordinator(context, thirdDb.absolutePath).use { sharing ->
            assertTrue(sharing.advanceRecipient(third).ready)
            assertTrue(!sharing.isAdmittedManager(third))
            assertTrue(runCatching { sharing.invite(third, origin, 1u.toUByte()) }.isFailure)
        }
        ShareCoordinator(context, holderDb.absolutePath).use { sharing ->
            val rotated = sharing.removeDevice(holder, origin, third.deviceId)
            assertEquals(2, rotated.devices.size)
            assertTrue(rotated.devices.none { it.deviceId.contentEquals(third.deviceId) })
        }
        ShareCoordinator(context, thirdDb.absolutePath).use { sharing ->
            assertTrue(sharing.syncRecipient(third).removed)
            assertTrue(runCatching { sharing.snapshot(third) }.isFailure)
            val copy = sharing.privateCopy(third, System.currentTimeMillis())
            NativeLocalStore.open(thirdDb.absolutePath).use { local ->
                assertTrue(local.children(copy).any { it.name == "Holder child" })
                val screen = loadTrackerData(local, sharing,
                    third.familyId.joinToString("") { "%02x".format(it) }, null,
                    third.familyId.joinToString("") { "%02x".format(it) })
                assertTrue(screen.removedFamilies.any { it.familyId.contentEquals(third.familyId) })
                assertTrue(screen.families.none { it.familyId.contentEquals(third.familyId) })
                assertTrue(screen.recipients.none { it.familyId.contentEquals(third.familyId) })
                assertTrue(screen.families.any { it.familyId.contentEquals(copy.familyId) })
            }
        }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            assertTrue(sharing.syncAndUpload(manager, origin).ready)
            assertTrue(sharing.snapshot(manager).devices.none { it.deviceId.contentEquals(third.deviceId) })
            sharing.addChild(manager, "Manager offline before removal", System.currentTimeMillis())
        }
        ShareCoordinator(context, holderDb.absolutePath).use { sharing ->
            val rotatedAgain = sharing.removeDevice(holder, origin, manager.deviceId)
            assertEquals(1, rotatedAgain.devices.size)
            assertArrayEquals(holder.deviceId, rotatedAgain.devices.single().deviceId)
            sharing.addChild(holder, "After second rotation", System.currentTimeMillis())
            assertTrue(sharing.syncRecipientAndUpload(holder).ready)
            assertTrue(sharing.snapshot(holder).children.any { it.name == "After second rotation" })
        }
        ShareCoordinator(context, managerDb.absolutePath).use { sharing ->
            val removed = sharing.checkInitialManagerRemoval(manager, origin)
                ?: error("Original manager should verify its removal")
            val copy = removed.privateCopy ?: error("Offline work needs a private copy")
            assertTrue(runCatching { sharing.snapshot(manager) }.isFailure)
            NativeLocalStore.open(managerDb.absolutePath).use { local ->
                assertTrue(local.children(copy).any { it.name == "Manager offline before removal" })
                val screen = loadTrackerData(local, sharing,
                    manager.familyId.joinToString("") { "%02x".format(it) }, null, null)
                assertTrue(screen.removedFamilies.any { it.familyId.contentEquals(manager.familyId) })
                assertTrue(screen.families.none { it.familyId.contentEquals(manager.familyId) })
                assertTrue(screen.families.any { it.familyId.contentEquals(copy.familyId) })
                assertArrayEquals(copy.familyId,
                    sharing.privateCopy(manager, System.currentTimeMillis()).familyId)
            }
        }
    }

    @Test
    fun keystoreWrappedPromotionAndInvitationSurviveRestart() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val publicKey = InstrumentationRegistry.getArguments().getString("relayPublicKey")
            ?: error("relayPublicKey instrumentation argument required")
        val origin = "http://localhost:8787"
        val firstKey = DeviceWrappingKey(context).loadOrCreate()
        val reopenedKey = DeviceWrappingKey(context).loadOrCreate()
        assertArrayEquals(firstKey, reopenedKey)
        firstKey.fill(0)
        reopenedKey.fill(0)

        val database = context.filesDir.resolve("sharing-test-${System.nanoTime()}.db")
        val family = NativeLocalStore.open(database.absolutePath).use { local ->
            val family = local.createFamily(System.currentTimeMillis())
            local.addChild(family, "Relay test child", System.currentTimeMillis())
            family
        }
        val fragment = ShareCoordinator(context, database.absolutePath).use { sharing ->
            assertEquals(1uL, sharing.promote(family, origin, publicKey))
            assertEquals("Relay test child", sharing.snapshot(family).children.single().name)
            assertEquals(1, sharing.snapshot(family).devices.size)
            NativeLocalStore.open(database.absolutePath).use { local ->
                assertTrue(runCatching { local.addChild(family, "Wrong surface", System.currentTimeMillis()) }.isFailure)
            }
            val fragment = sharing.invite(family, origin, 1u.toUByte())
            assertTrue(fragment.startsWith("#bt-invite=v1."))
            fragment
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            assertEquals(2uL, sharing.promote(family, origin, publicKey))
            assertTrue(sharing.invite(family, origin, 1u.toUByte()).startsWith("#bt-invite=v1."))
        }
        val recipient = context.filesDir.resolve("recipient-test-${System.nanoTime()}.db")
        val first = ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            sharing.claim(fragment)
        }
        assertArrayEquals(family.familyId, first.family.familyId)
        val retried = ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            sharing.claim(fragment)
        }
        assertEquals(first.family.deviceId.toList(), retried.family.deviceId.toList())
        assertArrayEquals(first.candidateBytes, retried.candidateBytes)
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            assertArrayEquals(first.family.familyId, sharing.recipientFamilies().single().familyId)
            assertEquals(origin, sharing.recipientOrigin(first.family))
        }
        NativeLocalStore.open(recipient.absolutePath).use { local ->
            assertTrue(local.families().none { it.familyId.contentEquals(family.familyId) })
            assertTrue(
                runCatching {
                    local.addChild(first.family, "Too early", System.currentTimeMillis())
                }.isFailure,
            )
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            sharing.respondToClaim(family, origin)
        }
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            sharing.proveChallenge(first.family)
        }
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            val pending = sharing.syncRecipient(first.family)
            assertTrue(pending.awaitingGrant)
            assertTrue(!pending.ready)
            assertTrue(runCatching { sharing.snapshot(first.family) }.isFailure)
            assertTrue(runCatching { sharing.addChild(first.family, "Too early", System.currentTimeMillis()) }.isFailure)
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            sharing.admitProvedDevice(family, origin)
        }
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            val synced = sharing.syncRecipient(first.family)
            assertTrue(synced.ready)
            assertEquals(1uL, synced.childCount)
            val existing = sharing.snapshot(first.family).children.single()
            assertEquals("Relay test child", existing.name)
            assertEquals(2, sharing.snapshot(first.family).devices.size)
            assertTrue(sharing.snapshot(first.family).devices.any {
                it.deviceId.contentEquals(first.family.deviceId) && it.role == 1u.toUByte()
            })
            sharing.addChild(first.family, "Offline shared child", System.currentTimeMillis())
            val now = System.currentTimeMillis()
            assertTrue(runCatching {
                sharing.logDiaper(first.family, ByteArray(16), 1u.toUByte(), ActivityWhen(now, 0, now))
            }.isFailure)
            sharing.logDiaper(first.family, existing.id, 1u.toUByte(), ActivityWhen(now, 0, now))
            assertEquals(2, sharing.snapshot(first.family).children.size)
            assertEquals(1, sharing.snapshot(first.family).activities.size)
            assertEquals(2uL, sharing.snapshot(first.family).unsentCount)
            assertEquals(0uL, sharing.snapshot(first.family).inertCount)
            val file = sharing.backupFile(first.family, now, null, 512_000_000uL)
            assertArrayEquals(first.family.familyId, file.info.sourceFamilyId)
            assertEquals(4uL, file.info.recordCount)
            val protected = sharing.backupFile(first.family, now, "backup secret", 512_000_000uL)
            NativeLocalStore.open(context.filesDir.resolve("restored-${System.nanoTime()}.db").absolutePath).use { local ->
                assertEquals(4uL, local.inspectProtected(protected.bytes, "backup secret", 512_000_000uL).recordCount)
                assertTrue(runCatching {
                    local.inspectProtected(protected.bytes, "wrong secret", 512_000_000uL)
                }.isFailure)
                val restored = local.restore(file.bytes, now)
                assertEquals(2, local.children(restored).size)
            }
            val copy = sharing.privateCopy(first.family, now)
            assertArrayEquals(copy.familyId, sharing.privateCopy(first.family, now + 1).familyId)
            assertTrue(!sharing.isShared(copy))
            NativeLocalStore.open(recipient.absolutePath).use { local ->
                assertEquals(2, local.children(copy).size)
            }
        }
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            assertTrue(sharing.syncRecipient(first.family).ready)
            assertEquals(2, sharing.snapshot(first.family).children.size)
            assertEquals(1, sharing.snapshot(first.family).activities.size)
            val wrapping = DeviceWrappingKey(context).loadOrCreate()
            try {
                NativeSharedStore.open(recipient.absolutePath).use { core ->
                    val candidate = core.prepareSharedUpload(first.family, wrapping)
                        ?: error("Expected an offline batch")
                    val familyHex = first.family.familyId.joinToString("") { "%02x".format(it.toInt() and 255) }
                    RelayTransport(origin).post("/v1/families/$familyHex/batches", candidate)
                    // Drop the response: the next sync must discover its signed acceptance.
                }
            } finally {
                wrapping.fill(0)
            }
            val uploaded = sharing.syncAndUpload(first.family, origin)
            assertTrue(uploaded.ready)
            assertEquals(0u.toUByte(), uploaded.outboxState)
            assertEquals(0uL, sharing.snapshot(first.family).unsentCount)
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            assertTrue(sharing.syncAndUpload(family, origin).ready)
            assertEquals(2, sharing.snapshot(family).children.size)
            assertEquals(1, sharing.snapshot(family).activities.size)
            sharing.addChild(family, "Manager later", System.currentTimeMillis())
            assertTrue(sharing.syncAndUpload(family, origin).ready)
        }
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            assertTrue(sharing.syncAndUpload(first.family, origin).ready)
            assertEquals(3, sharing.snapshot(first.family).children.size)
            sharing.addChild(first.family, "Saved at removal", System.currentTimeMillis())
            assertEquals(1uL, sharing.snapshot(first.family).unsentCount)
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            sharing.admitProvedDevice(family, origin)
        }
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            sharing.proveChallenge(fragment)
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            sharing.respondToClaim(family, origin)
        }
        ShareCoordinator(context, database.absolutePath).use { sharing ->
            val rotated = sharing.removeDevice(family, origin, first.family.deviceId)
            assertEquals(1, rotated.devices.size)
            assertTrue(rotated.devices.single().deviceId.contentEquals(family.deviceId))
            val child = rotated.children.first()
            val now = System.currentTimeMillis()
            sharing.logDiaper(family, child.id, 2u.toUByte(), ActivityWhen(now, 0, now))
            assertTrue(sharing.syncAndUpload(family, origin).ready)
            assertEquals(0uL, sharing.snapshot(family).unsentCount)
        }
        ShareCoordinator(context, recipient.absolutePath).use { sharing ->
            val removed = sharing.advanceRecipient(first.family)
            assertTrue(removed.removed)
            val copy = removed.privateCopy ?: error("Pending edit needs an independent copy")
            assertTrue(!sharing.isShared(copy))
            NativeLocalStore.open(recipient.absolutePath).use { local ->
                assertTrue(local.children(copy).any { it.name == "Saved at removal" })
            }
            assertArrayEquals(copy.familyId, sharing.advanceRecipient(first.family).privateCopy!!.familyId)
            assertTrue(runCatching { sharing.syncAndUpload(first.family, origin) }.isFailure)
        }
    }
}
