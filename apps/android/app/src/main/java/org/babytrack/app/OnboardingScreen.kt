package org.babytrack.app

import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp

@Composable
internal fun OnboardingWelcomeScreen(
    onAddChild: () -> Unit = {},
    onJoin: () -> Unit = {},
    onRestore: () -> Unit = {},
    scrollState: ScrollState = rememberScrollState(),
) {
    var showPrivacy by remember { mutableStateOf(false) }
    Scaffold(
        bottomBar = {
            Surface(Modifier.navigationBarsPadding()) {
                Column(
                    Modifier.fillMaxWidth().padding(20.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Button(onClick = onAddChild, Modifier.fillMaxWidth().heightIn(min = 56.dp)) {
                        Text(stringResource(R.string.onboarding_add_child))
                    }
                    OutlinedButton(onClick = onJoin, Modifier.fillMaxWidth().heightIn(min = 48.dp)) {
                        Text(stringResource(R.string.join_family))
                    }
                    TextButton(onClick = onRestore, Modifier.fillMaxWidth().heightIn(min = 48.dp)) {
                        Text(stringResource(R.string.restore_backup))
                    }
                }
            }
        },
    ) { padding: PaddingValues ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(scrollState)
                .padding(horizontal = 24.dp, vertical = 32.dp),
            verticalArrangement = Arrangement.spacedBy(24.dp),
        ) {
            Text(
                stringResource(R.string.onboarding_title),
                style = MaterialTheme.typography.headlineLarge,
            )
            TextButton(onClick = { showPrivacy = true }, modifier = Modifier.heightIn(min = 48.dp)) {
                Text(stringResource(R.string.onboarding_privacy_link))
            }
        }
    }
    if (showPrivacy) {
        AlertDialog(
            onDismissRequest = { showPrivacy = false },
            title = { Text(stringResource(R.string.onboarding_privacy_title)) },
            text = {
                Text(
                    stringResource(R.string.onboarding_privacy_body),
                    modifier = Modifier.heightIn(max = 360.dp).verticalScroll(rememberScrollState()),
                )
            },
            confirmButton = {
                TextButton(onClick = { showPrivacy = false }) {
                    Text(stringResource(R.string.onboarding_got_it))
                }
            },
        )
    }
}
