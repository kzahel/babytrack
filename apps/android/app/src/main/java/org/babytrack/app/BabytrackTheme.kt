package org.babytrack.app

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp

// Calm neutral surfaces leave the strongest color for actions and status.
// Activity colors are specified in the interface design topic and will be
// applied when the Today and capture screens are split into components.
private val LightColors = lightColorScheme(
    primary = Color(0xFF176B75),
    onPrimary = Color.White,
    primaryContainer = Color(0xFFD3F0EE),
    onPrimaryContainer = Color(0xFF153B40),
    secondary = Color(0xFF41636A),
    onSecondary = Color.White,
    secondaryContainer = Color(0xFFD9E8E5),
    onSecondaryContainer = Color(0xFF19383E),
    tertiary = Color(0xFF9E4B30),
    onTertiary = Color.White,
    tertiaryContainer = Color(0xFFFFD5C5),
    onTertiaryContainer = Color(0xFF482217),
    background = Color(0xFFF8F7F3),
    onBackground = Color(0xFF18272C),
    surface = Color(0xFFFFFFFF),
    onSurface = Color(0xFF18272C),
    surfaceTint = Color(0xFF176B75),
    surfaceVariant = Color(0xFFE9F0EE),
    onSurfaceVariant = Color(0xFF31474D),
    surfaceContainerLowest = Color(0xFFFFFFFF),
    surfaceContainerLow = Color(0xFFF6F7F4),
    surfaceContainer = Color(0xFFF0F3EF),
    surfaceContainerHigh = Color(0xFFE8EEEB),
    surfaceContainerHighest = Color(0xFFE1E9E6),
    outline = Color(0xFF647A7D),
)

private val DarkColors = darkColorScheme(
    primary = Color(0xFFA5E7E5),
    onPrimary = Color(0xFF113238),
    primaryContainer = Color(0xFF24545B),
    onPrimaryContainer = Color(0xFFD3F0EE),
    secondary = Color(0xFFAFCED0),
    onSecondary = Color(0xFF18363C),
    secondaryContainer = Color(0xFF344E54),
    onSecondaryContainer = Color(0xFFE0F0EE),
    tertiary = Color(0xFFFFC0A8),
    onTertiary = Color(0xFF482217),
    tertiaryContainer = Color(0xFF75402E),
    onTertiaryContainer = Color(0xFFFFD5C5),
    background = Color(0xFF111C22),
    onBackground = Color(0xFFF6F7F4),
    surface = Color(0xFF192A30),
    onSurface = Color(0xFFF6F7F4),
    surfaceTint = Color(0xFFA5E7E5),
    surfaceVariant = Color(0xFF31474D),
    onSurfaceVariant = Color(0xFFD8E5E2),
    surfaceContainerLowest = Color(0xFF0C171C),
    surfaceContainerLow = Color(0xFF17252B),
    surfaceContainer = Color(0xFF1F3036),
    surfaceContainerHigh = Color(0xFF2A3D43),
    surfaceContainerHighest = Color(0xFF354A50),
    outline = Color(0xFF9CB4B5),
)

private val BabytrackShapes = Shapes(
    small = RoundedCornerShape(12.dp),
    medium = RoundedCornerShape(18.dp),
    large = RoundedCornerShape(24.dp),
)

@Composable
internal fun BabytrackTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = if (isSystemInDarkTheme()) DarkColors else LightColors,
        shapes = BabytrackShapes,
        content = content,
    )
}
