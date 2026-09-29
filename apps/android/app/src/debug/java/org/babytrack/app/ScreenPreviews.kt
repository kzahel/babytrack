package org.babytrack.app

import androidx.compose.foundation.rememberScrollState
import androidx.compose.runtime.Composable
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.tooling.preview.PreviewParameter
import androidx.compose.ui.tooling.preview.PreviewParameterProvider

internal class ScreenFixtureProvider : PreviewParameterProvider<ScreenFixture> {
    override val values = ScreenFixtures.cases.asSequence()
}

@Preview(
    name = "Dark",
    widthDp = 360,
    heightDp = 800,
    uiMode = android.content.res.Configuration.UI_MODE_NIGHT_YES,
    locale = "en-rUS",
)
@Preview(
    name = "Light",
    widthDp = 360,
    heightDp = 800,
    uiMode = android.content.res.Configuration.UI_MODE_NIGHT_NO,
    locale = "en-rUS",
)
@Preview(
    name = "Dark · large text",
    widthDp = 360,
    heightDp = 800,
    fontScale = 1.5f,
    uiMode = android.content.res.Configuration.UI_MODE_NIGHT_YES,
    locale = "en-rUS",
)
@Preview(
    name = "Light · large text",
    widthDp = 360,
    heightDp = 800,
    fontScale = 1.5f,
    uiMode = android.content.res.Configuration.UI_MODE_NIGHT_NO,
    locale = "en-rUS",
)
@Composable
internal fun ScreenGalleryPreview(
    @PreviewParameter(ScreenFixtureProvider::class) fixture: ScreenFixture
) {
    BabytrackTheme { FixtureScreen(fixture, rememberScrollState()) }
}
