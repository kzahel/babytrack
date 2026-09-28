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
    secondary = Color(0xFF9E4B30),
    onSecondary = Color.White,
    secondaryContainer = Color(0xFFFFD5C5),
    onSecondaryContainer = Color(0xFF482217),
    tertiary = Color(0xFF603F83),
    onTertiary = Color.White,
    tertiaryContainer = Color(0xFFEEE7F8),
    onTertiaryContainer = Color(0xFF2A1D3D),
    background = Color(0xFFF8F7F3),
    onBackground = Color(0xFF18272C),
    surface = Color(0xFFFFFFFF),
    onSurface = Color(0xFF18272C),
    surfaceVariant = Color(0xFFE9F0EE),
    onSurfaceVariant = Color(0xFF31474D),
    outline = Color(0xFF647A7D),
)

private val DarkColors = darkColorScheme(
    primary = Color(0xFFA5E7E5),
    onPrimary = Color(0xFF113238),
    primaryContainer = Color(0xFF24545B),
    onPrimaryContainer = Color(0xFFD3F0EE),
    secondary = Color(0xFFFFC0A8),
    onSecondary = Color(0xFF482217),
    secondaryContainer = Color(0xFF75402E),
    onSecondaryContainer = Color(0xFFFFD5C5),
    tertiary = Color(0xFFD8C4F4),
    onTertiary = Color(0xFF2A1D3D),
    tertiaryContainer = Color(0xFF4E3A67),
    onTertiaryContainer = Color(0xFFEEE7F8),
    background = Color(0xFF111C22),
    onBackground = Color(0xFFF6F7F4),
    surface = Color(0xFF192A30),
    onSurface = Color(0xFFF6F7F4),
    surfaceVariant = Color(0xFF31474D),
    onSurfaceVariant = Color(0xFFD8E5E2),
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
