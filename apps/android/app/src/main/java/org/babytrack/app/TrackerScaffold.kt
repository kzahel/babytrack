package org.babytrack.app

import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.ui.Alignment
import androidx.compose.ui.draw.clip
import androidx.compose.ui.unit.Dp
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

internal data class TrackerChromeState(
    val route: TrackerDestination,
    val title: String,
    val hasChild: Boolean,
    val canNavigate: Boolean,
    /** Shown under the child's name only when it distinguishes a Family. */
    val subtitle: String? = null,
    /** The open capture form's activity kind, for its badge. */
    val titleKind: String? = null,
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun TrackerScaffold(
    state: TrackerChromeState,
    scrollState: ScrollState,
    snackbarHostState: SnackbarHostState = remember { SnackbarHostState() },
    onNavigate: (TrackerDestination) -> Unit = {},
    onBack: () -> Unit = {},
    onSwitchTarget: () -> Unit = {},
    bottomAction: (@Composable RowScope.() -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    with(state) {
        Scaffold(
            topBar = {
                TopAppBar(
                    navigationIcon = {
                        if (route == TrackerDestination.CAPTURE) {
                            IconButton(onClick = onBack) {
                                Icon(
                                    painterResource(R.drawable.ic_back),
                                    contentDescription = stringResource(R.string.back),
                                )
                            }
                        }
                    },
                    title = {
                        if (hasChild && route != TrackerDestination.CAPTURE) {
                            val description = stringResource(R.string.switch_target)
                            TextButton(
                                onClick = onSwitchTarget,
                                modifier = Modifier.semantics { contentDescription = description },
                            ) {
                                ChildAvatar(title)
                                Column(Modifier.padding(start = 12.dp)) {
                                    Text(
                                        title,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                        color = MaterialTheme.colorScheme.onSurface,
                                        style = MaterialTheme.typography.titleLarge,
                                    )
                                    subtitle?.let {
                                        Text(
                                            it,
                                            maxLines = 1,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                            style = MaterialTheme.typography.labelMedium,
                                        )
                                    }
                                }
                                Icon(
                                    painterResource(R.drawable.ic_expand_more),
                                    contentDescription = null,
                                )
                            }
                        } else
                            Row(
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(12.dp),
                            ) {
                                titleKind?.let { ActivityBadge(it, size = 36.dp) }
                                Column {
                                    Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                    subtitle?.let {
                                        Text(
                                            it,
                                            maxLines = 1,
                                            overflow = TextOverflow.Ellipsis,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                            style = MaterialTheme.typography.labelMedium,
                                        )
                                    }
                                }
                            }
                    },
                    colors =
                        TopAppBarDefaults.topAppBarColors(
                            containerColor = MaterialTheme.colorScheme.background
                        ),
                )
            },
            containerColor = MaterialTheme.colorScheme.background,
            bottomBar = {
                if (route == TrackerDestination.CAPTURE && bottomAction != null) {
                    BottomActionBar(bottomAction)
                } else if (canNavigate && route != TrackerDestination.CAPTURE) {
                    NavigationBar {
                        listOf(
                                Triple(
                                    TrackerDestination.TODAY,
                                    R.string.nav_today,
                                    R.drawable.ic_today,
                                ),
                                Triple(
                                    TrackerDestination.HISTORY,
                                    R.string.nav_history,
                                    R.drawable.ic_history,
                                ),
                                Triple(
                                    TrackerDestination.FAMILY,
                                    R.string.nav_family,
                                    R.drawable.ic_family,
                                ),
                            )
                            .forEach { (target, label, icon) ->
                                NavigationBarItem(
                                    selected = route == target,
                                    onClick = { onNavigate(target) },
                                    icon = {
                                        Icon(painterResource(icon), contentDescription = null)
                                    },
                                    label = { Text(stringResource(label)) },
                                )
                            }
                    }
                }
            },
            snackbarHost = { SnackbarHost(snackbarHostState) },
        ) { padding ->
            Column(
                modifier =
                    Modifier.fillMaxSize()
                        .padding(padding)
                        .verticalScroll(scrollState)
                        .padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
                content = content,
            )
        }
    }
}

/** A child's initial in a quiet circle; the name beside it is the accessible label. */
@Composable
internal fun ChildAvatar(name: String, size: Dp = 36.dp) {
    Box(
        Modifier.size(size).clip(CircleShape).background(MaterialTheme.colorScheme.primaryContainer),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            name.trim().firstOrNull()?.uppercase() ?: "",
            color = MaterialTheme.colorScheme.onPrimaryContainer,
            style = MaterialTheme.typography.titleMedium,
        )
    }
}
