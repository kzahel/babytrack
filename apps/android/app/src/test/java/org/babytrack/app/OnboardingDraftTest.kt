package org.babytrack.app

import android.app.Application
import android.content.Context
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import uniffi.babytrack_core_ffi.FamilyRef

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], application = Application::class)
class OnboardingDraftTest {
    @Test fun retryAfterChildWriteFailureReusesDurableFamilyAndDraft() {
        val prefs = RuntimeEnvironment.getApplication().getSharedPreferences("onboarding_test", Context.MODE_PRIVATE)
        prefs.edit().clear().commit()
        val draft = OnboardingDraft(prefs)
        draft.updateName("Rowan")
        draft.updateBirthDate("2026-06-29")
        draft.updateOpen(true)
        val family = FamilyRef(ByteArray(16) { 1 }, ByteArray(16) { 2 })
        var creations = 0
        assertThrows(IllegalStateException::class.java) {
            finishLocalOnboarding(null, { creations++; family }, draft::rememberFamily) { error("disk full") }
        }
        val reopened = OnboardingDraft(prefs)
        assertEquals("Rowan", reopened.name)
        assertEquals("2026-06-29", reopened.birthDate)
        assertTrue(reopened.open)
        assertEquals(family.familyId.key(), reopened.stagedFamilyKey)
        val result = finishLocalOnboarding(family, { creations++; family }, reopened::rememberFamily) { ByteArray(16) { 3 } }
        assertEquals(1, creations)
        assertSame(family, result.first)
        reopened.clear()
        val finished = OnboardingDraft(prefs)
        assertFalse(finished.open)
        assertEquals("", finished.name)
        assertNull(finished.stagedFamilyKey)
    }

    @Test fun backingOutPreservesInputWithoutStagingAFamily() {
        val prefs = RuntimeEnvironment.getApplication().getSharedPreferences("onboarding_back_test", Context.MODE_PRIVATE)
        prefs.edit().clear().commit()
        val draft = OnboardingDraft(prefs)
        draft.updateOpen(true)
        draft.updateName("Nickname")
        draft.updateOpen(false)
        val reopened = OnboardingDraft(prefs)
        assertFalse(reopened.open)
        assertEquals("Nickname", reopened.name)
        assertNull(reopened.stagedFamilyKey)
    }
}
