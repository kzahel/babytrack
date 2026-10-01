package org.babytrack.app

import android.content.SharedPreferences
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import uniffi.babytrack_core_ffi.FamilyRef

/** UI draft only; canonical Family and child writes still belong to the Rust core. */
internal class OnboardingDraft(private val prefs: SharedPreferences) {
    var name by mutableStateOf(prefs.getString("name", "").orEmpty())
        private set
    var birthDate by mutableStateOf(prefs.getString("birth_date", "").orEmpty())
        private set
    var open by mutableStateOf(prefs.getBoolean("open", false))
        private set
    var saving by mutableStateOf(false)
    var error by mutableStateOf<String?>(null)
    val stagedFamilyKey: String? get() = prefs.getString("family", null)

    fun updateName(value: String) {
        name = value
        error = null
        prefs.edit().putString("name", value).apply()
    }
    fun updateBirthDate(value: String) {
        birthDate = value
        error = null
        prefs.edit().putString("birth_date", value).apply()
    }
    fun updateOpen(value: Boolean) {
        open = value
        prefs.edit().putBoolean("open", value).apply()
    }
    // Persist before adding a child so a retry after a partial write reuses the Family.
    fun rememberFamily(family: FamilyRef) {
        check(prefs.edit().putString("family", family.familyId.key()).commit())
    }
    fun clear() {
        name = ""
        birthDate = ""
        open = false
        error = null
        prefs.edit().clear().apply()
    }
}

/** Native orchestration, not a second implementation of event or storage semantics. */
internal fun finishLocalOnboarding(
    existingFamily: FamilyRef?,
    createFamily: () -> FamilyRef,
    rememberFamily: (FamilyRef) -> Unit,
    addChild: (FamilyRef) -> ByteArray,
): Pair<FamilyRef, ByteArray> {
    val family = existingFamily ?: createFamily()
    rememberFamily(family)
    return family to addChild(family)
}
