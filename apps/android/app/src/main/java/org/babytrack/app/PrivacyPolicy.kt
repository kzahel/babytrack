package org.babytrack.app

import androidx.compose.foundation.layout.heightIn
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp

@Composable
internal fun PrivacyPolicyLink() {
    val uriHandler = LocalUriHandler.current
    val url = stringResource(R.string.privacy_policy_url)
    TextButton(onClick = { uriHandler.openUri(url) }, modifier = Modifier.heightIn(min = 48.dp)) {
        Text(stringResource(R.string.full_privacy_policy))
    }
}
