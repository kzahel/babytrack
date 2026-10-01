package org.babytrack.app

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.ChevronRight
import androidx.compose.material.icons.outlined.ExpandLess
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.material.icons.outlined.Schedule
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay

/** Shared presentation pieces. They render supplied state and call back; no store access. */

@Composable
internal fun categoryColors(kind: String): CategoryColors =
    activityCategory(kind)?.let { LocalActivityPalette.current[it] }
        ?: CategoryColors(
            MaterialTheme.colorScheme.onSurfaceVariant,
            MaterialTheme.colorScheme.surfaceVariant,
        )

/** The activity's icon in its category container. Decorative: pair it with a text label. */
@Composable
internal fun ActivityBadge(kind: String, modifier: Modifier = Modifier, size: Dp = 40.dp) {
    val colors = categoryColors(kind)
    Box(
        modifier
            .size(size)
            .clip(RoundedCornerShape(size * 0.3f))
            .background(colors.container),
        contentAlignment = Alignment.Center,
    ) {
        Icon(
            activityIcon(kind),
            contentDescription = null,
            tint = colors.accent,
            modifier = Modifier.size(size * 0.58f),
        )
    }
}

/** One compact timeline row: icon, title, optional detail lines, and a trailing time. */
@Composable
internal fun EntryRow(
    kind: String,
    title: String,
    time: String,
    modifier: Modifier = Modifier,
    details: List<String> = emptyList(),
    onClickLabel: String? = null,
    onClick: (() -> Unit)? = null,
) {
    Row(
        modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .clip(MaterialTheme.shapes.small)
            .then(
                if (onClick != null) Modifier.clickable(onClickLabel = onClickLabel, onClick = onClick)
                else Modifier
            )
            .padding(horizontal = 4.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        ActivityBadge(kind)
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.titleSmall)
            details.forEach {
                Text(
                    it,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        Text(
            time,
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.End,
        )
    }
}

@Composable
internal fun SectionHeader(
    text: String,
    modifier: Modifier = Modifier,
    actionLabel: String? = null,
    onAction: () -> Unit = {},
) {
    Row(
        modifier.fillMaxWidth().padding(top = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text,
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.SemiBold,
            modifier = Modifier.weight(1f),
        )
        if (actionLabel != null) TextButton(onClick = onAction) { Text(actionLabel) }
    }
}

/** Large selectable tiles with an icon and label, sized for one-handed taps. */
@Composable
internal fun <T> ChoiceTiles(
    options: List<Triple<T, ImageVector, String>>,
    selected: T?,
    onSelect: (T) -> Unit,
    modifier: Modifier = Modifier,
) {
    Row(modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        options.forEach { (value, icon, label) ->
            val isSelected = value == selected
            Surface(
                shape = MaterialTheme.shapes.medium,
                color =
                    if (isSelected) MaterialTheme.colorScheme.primaryContainer
                    else MaterialTheme.colorScheme.surfaceContainer,
                border =
                    if (isSelected) BorderStroke(2.dp, MaterialTheme.colorScheme.primary)
                    else null,
                modifier =
                    Modifier.weight(1f)
                        .heightIn(min = 96.dp)
                        .clip(MaterialTheme.shapes.medium)
                        .selectable(
                            selected = isSelected,
                            role = Role.RadioButton,
                            onClick = { onSelect(value) },
                        ),
            ) {
                Column(
                    Modifier.padding(vertical = 12.dp, horizontal = 4.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(6.dp, Alignment.CenterVertically),
                ) {
                    Icon(
                        icon,
                        contentDescription = null,
                        tint =
                            if (isSelected) MaterialTheme.colorScheme.onPrimaryContainer
                            else MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.size(32.dp),
                    )
                    Text(
                        label,
                        style = MaterialTheme.typography.labelLarge,
                        textAlign = TextAlign.Center,
                    )
                }
            }
        }
    }
}

/** A single row of choices that scrolls sideways when labels grow. */
@Composable
internal fun <T> ChipRow(
    options: List<Pair<T, String>>,
    selected: T?,
    onSelect: (T) -> Unit,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        options.forEach { (value, label) ->
            FilterChip(
                selected = value == selected,
                onClick = { onSelect(value) },
                label = { Text(label) },
            )
        }
    }
}

/** Two or three short, mutually exclusive choices such as units or sides. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun <T> SegmentedChoice(
    options: List<Pair<T, String>>,
    selected: T,
    onSelect: (T) -> Unit,
    modifier: Modifier = Modifier,
) {
    SingleChoiceSegmentedButtonRow(modifier.fillMaxWidth()) {
        options.forEachIndexed { index, (value, label) ->
            SegmentedButton(
                selected = value == selected,
                onClick = { onSelect(value) },
                shape = SegmentedButtonDefaults.itemShape(index, options.size),
                label = { Text(label, textAlign = TextAlign.Center) },
            )
        }
    }
}

/** The time a new entry will use, with one tap to choose another. */
@Composable
internal fun TimeRow(
    value: String,
    chooseLabel: String,
    onChoose: () -> Unit,
    modifier: Modifier = Modifier,
    resetLabel: String? = null,
    onReset: () -> Unit = {},
) {
    Surface(
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.surfaceContainer,
        modifier = modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(start = 16.dp, end = 8.dp, top = 8.dp, bottom = 8.dp)) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Icon(
                    Icons.Outlined.Schedule,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    value,
                    style = MaterialTheme.typography.bodyLarge,
                    modifier = Modifier.weight(1f),
                )
                if (resetLabel == null) TextButton(onClick = onChoose) { Text(chooseLabel) }
            }
            if (resetLabel != null)
                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.End,
                ) {
                    TextButton(onClick = onReset) { Text(resetLabel) }
                    TextButton(onClick = onChoose) { Text(chooseLabel) }
                }
        }
    }
}

/** A fixed footer for a form's one main action; it rises above the keyboard. */
@Composable
internal fun BottomActionBar(content: @Composable RowScope.() -> Unit) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainer,
        tonalElevation = 3.dp,
        modifier = Modifier.navigationBarsPadding().imePadding(),
    ) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
            content = content,
        )
    }
}

@Composable
internal fun RowScope.PrimaryAction(text: String, enabled: Boolean = true, onClick: () -> Unit) {
    Button(
        onClick = onClick,
        enabled = enabled,
        modifier = Modifier.weight(1f).heightIn(min = 56.dp),
    ) {
        Text(text, style = MaterialTheme.typography.titleMedium)
    }
}

/** A quiet status label; warnings use the error role and stay explicit in text. */
@Composable
internal fun StatusChip(text: String, icon: ImageVector, warning: Boolean = false) {
    Surface(
        shape = RoundedCornerShape(50),
        color =
            if (warning) MaterialTheme.colorScheme.errorContainer
            else MaterialTheme.colorScheme.surfaceContainerHigh,
        contentColor =
            if (warning) MaterialTheme.colorScheme.onErrorContainer
            else MaterialTheme.colorScheme.onSurfaceVariant,
    ) {
        Row(
            Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Icon(icon, contentDescription = null, modifier = Modifier.size(16.dp))
            Text(text, style = MaterialTheme.typography.labelLarge)
        }
    }
}

/** A settings-style row: icon, title, one line of supporting text, and a chevron. */
@Composable
internal fun SettingsRow(
    icon: ImageVector,
    title: String,
    supporting: String?,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    expanded: Boolean? = null,
) {
    Row(
        modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .clip(MaterialTheme.shapes.small)
            .clickable(onClick = onClick)
            .padding(horizontal = 4.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Icon(icon, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.titleSmall)
            if (!supporting.isNullOrBlank())
                Text(
                    supporting,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
        }
        Icon(
            if (expanded == true) Icons.Outlined.ExpandLess
            else if (expanded == false) Icons.Outlined.ExpandMore
            else Icons.Outlined.ChevronRight,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

/** A grouped surface for a set of related rows. */
@Composable
internal fun SectionCard(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    Surface(
        shape = MaterialTheme.shapes.large,
        color = MaterialTheme.colorScheme.surface,
        modifier = modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(horizontal = 12.dp, vertical = 4.dp)) { content() }
    }
}

/**
 * The instant for elapsed labels. Fixtures pass live = false and keep the
 * supplied instant; the running app advances it at [tickMs].
 */
@Composable
internal fun rememberNow(initial: Long, live: Boolean, tickMs: Long): Long {
    val now by
        produceState(initial, live, tickMs) {
            if (live)
                while (true) {
                    value = System.currentTimeMillis()
                    delay(tickMs - value % tickMs)
                }
        }
    return now
}
