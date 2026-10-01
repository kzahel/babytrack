package org.babytrack.app

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.BabyChangingStation
import androidx.compose.material.icons.outlined.Bedtime
import androidx.compose.material.icons.outlined.ChildCare
import androidx.compose.material.icons.outlined.HelpOutline
import androidx.compose.material.icons.outlined.LocalDrink
import androidx.compose.material.icons.outlined.Medication
import androidx.compose.material.icons.outlined.Opacity
import androidx.compose.material.icons.outlined.Restaurant
import androidx.compose.material.icons.outlined.StickyNote2
import androidx.compose.material.icons.outlined.Straighten
import androidx.compose.material.icons.outlined.Thermostat
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector

/** Presentation grouping only; event kinds and their meaning stay in the Rust core. */
internal enum class ActivityCategory {
    FEED,
    SLEEP,
    DIAPER,
    HEALTH,
    NOTE,
}

@Immutable internal data class CategoryColors(val accent: Color, val container: Color)

@Immutable
internal data class ActivityPalette(
    val feed: CategoryColors,
    val sleep: CategoryColors,
    val diaper: CategoryColors,
    val health: CategoryColors,
    val note: CategoryColors,
) {
    operator fun get(category: ActivityCategory): CategoryColors =
        when (category) {
            ActivityCategory.FEED -> feed
            ActivityCategory.SLEEP -> sleep
            ActivityCategory.DIAPER -> diaper
            ActivityCategory.HEALTH -> health
            ActivityCategory.NOTE -> note
        }
}

// Accents are icon/text colors on their container; tile text uses the theme ink.
internal val LightActivityPalette =
    ActivityPalette(
        feed = CategoryColors(Color(0xFF9A4317), Color(0xFFFFE4D4)),
        sleep = CategoryColors(Color(0xFF34558F), Color(0xFFDFE7F8)),
        diaper = CategoryColors(Color(0xFF3B6535), Color(0xFFDDEBD6)),
        health = CategoryColors(Color(0xFF68478F), Color(0xFFECE2F6)),
        note = CategoryColors(Color(0xFF66563A), Color(0xFFEFE8DA)),
    )

internal val DarkActivityPalette =
    ActivityPalette(
        feed = CategoryColors(Color(0xFFFFB892), Color(0xFF4F2A16)),
        sleep = CategoryColors(Color(0xFFB4C8F6), Color(0xFF243656)),
        diaper = CategoryColors(Color(0xFFB1D5A5), Color(0xFF26401F)),
        health = CategoryColors(Color(0xFFD7C0F4), Color(0xFF3D2C56)),
        note = CategoryColors(Color(0xFFE2D2AE), Color(0xFF423925)),
    )

internal val LocalActivityPalette = staticCompositionLocalOf { LightActivityPalette }

internal fun activityCategory(kind: String): ActivityCategory? =
    when (kind) {
        "feed.bottle",
        "feed.breast",
        "feed.solids",
        "pump" -> ActivityCategory.FEED
        "sleep" -> ActivityCategory.SLEEP
        "diaper" -> ActivityCategory.DIAPER
        "growth",
        "temperature",
        "medication" -> ActivityCategory.HEALTH
        "note" -> ActivityCategory.NOTE
        else -> null
    }

internal fun activityIcon(kind: String): ImageVector =
    when (kind) {
        "feed.bottle" -> Icons.Outlined.LocalDrink
        "feed.breast" -> Icons.Outlined.ChildCare
        "feed.solids" -> Icons.Outlined.Restaurant
        "pump" -> Icons.Outlined.Opacity
        "sleep" -> Icons.Outlined.Bedtime
        "diaper" -> Icons.Outlined.BabyChangingStation
        "growth" -> Icons.Outlined.Straighten
        "temperature" -> Icons.Outlined.Thermostat
        "medication" -> Icons.Outlined.Medication
        "note" -> Icons.Outlined.StickyNote2
        else -> Icons.Outlined.HelpOutline
    }

internal val CaptureKind.activityKind: String
    get() =
        when (this) {
            CaptureKind.DIAPER -> "diaper"
            CaptureKind.BOTTLE -> "feed.bottle"
            CaptureKind.BREAST -> "feed.breast"
            CaptureKind.PUMP -> "pump"
            CaptureKind.SOLIDS -> "feed.solids"
            CaptureKind.SLEEP -> "sleep"
            CaptureKind.GROWTH -> "growth"
            CaptureKind.TEMPERATURE -> "temperature"
            CaptureKind.MEDICATION -> "medication"
            CaptureKind.NOTE -> "note"
        }
