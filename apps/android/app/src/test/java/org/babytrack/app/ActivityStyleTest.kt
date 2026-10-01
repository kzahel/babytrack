package org.babytrack.app

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ActivityStyleTest {
    private fun contrast(a: Color, b: Color): Double {
        val (light, dark) = listOf(a.luminance(), b.luminance()).sortedDescending()
        return (light + 0.05) / (dark + 0.05)
    }

    @Test
    fun categoryColorsMeetContrastInBothThemes() {
        listOf(
                Triple("light", LightActivityPalette, LightColors),
                Triple("dark", DarkActivityPalette, DarkColors),
            )
            .forEach { (theme, palette, scheme) ->
                ActivityCategory.entries.forEach { category ->
                    val colors = palette[category]
                    // Icon accents are text-weight here, so hold them to the text threshold.
                    val accent = contrast(colors.accent, colors.container)
                    assertTrue("$theme $category accent $accent", accent >= 4.5)
                    val ink = contrast(scheme.onSurface, colors.container)
                    assertTrue("$theme $category ink $ink", ink >= 4.5)
                    val tile = contrast(colors.container, scheme.background)
                    assertTrue("$theme $category tile is distinct", tile >= 1.1)
                }
            }
    }

    @Test
    fun everyCaptureKindHasACategory() {
        CaptureKind.entries.forEach { assertNotNull(it.name, activityCategory(it.activityKind)) }
        assertNull(activityCategory("future.example"))
        assertEquals(ActivityCategory.FEED, activityCategory("pump"))
    }
}
