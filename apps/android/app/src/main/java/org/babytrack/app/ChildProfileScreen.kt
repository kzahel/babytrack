package org.babytrack.app

import android.app.DatePickerDialog
import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import java.text.DateFormat
import java.time.LocalDate
import java.time.Period
import java.time.ZoneId
import java.time.temporal.ChronoUnit
import java.util.Date

internal fun childAgeLabel(context: Context, birthDate: String): String {
    val birth =
        runCatching { LocalDate.parse(birthDate) }.getOrNull()
            ?: return context.getString(R.string.child_age_unknown)
    val today = LocalDate.now(ZoneId.systemDefault())
    if (birth.isAfter(today)) return context.getString(R.string.child_age_unknown)
    val period = Period.between(birth, today)
    val days = ChronoUnit.DAYS.between(birth, today).toInt()
    return when {
        days < 14 -> context.resources.getQuantityString(R.plurals.child_age_days, days, days)
        period.years == 0 && period.months == 0 -> {
            val weeks = days / 7
            context.resources.getQuantityString(R.plurals.child_age_weeks, weeks, weeks)
        }
        period.years < 2 -> {
            val months = period.years * 12 + period.months
            context.resources.getQuantityString(R.plurals.child_age_months, months, months)
        }
        else ->
            context.resources.getQuantityString(
                R.plurals.child_age_years,
                period.years,
                period.years,
            )
    }
}

internal fun birthDateLabel(context: Context, isoDate: String): String =
    runCatching {
            val instant = LocalDate.parse(isoDate).atStartOfDay(ZoneId.systemDefault()).toInstant()
            DateFormat.getDateInstance(DateFormat.MEDIUM).format(Date.from(instant))
        }
        .getOrElse { context.getString(R.string.birth_date_not_set) }

private fun chooseBirthDate(context: Context, isoDate: String, onSelected: (String) -> Unit) {
    val day = runCatching { LocalDate.parse(isoDate) }.getOrElse { LocalDate.now() }
    DatePickerDialog(
            context,
            { _, year, month, date -> onSelected(LocalDate.of(year, month + 1, date).toString()) },
            day.year,
            day.monthValue - 1,
            day.dayOfMonth,
        )
        .apply { datePicker.maxDate = System.currentTimeMillis() }
        .show()
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ChildProfileScreen(
    editing: Boolean,
    name: String,
    birthDate: String,
    sex: UByte,
    saving: Boolean,
    canClearBirthDate: Boolean,
    onNameChange: (String) -> Unit,
    onBirthDateChange: (String) -> Unit,
    onSexChange: (UByte) -> Unit,
    onSave: () -> Unit,
    onDismiss: () -> Unit,
) {
    val context = LocalContext.current
    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        ChildProfileContent(
            ChildProfileUiState(
                editing,
                name,
                birthDate,
                sex,
                saving,
                canClearBirthDate,
                childAgeLabel(context, birthDate),
            ),
            ChildProfileActions(
                onNameChange,
                { chooseBirthDate(context, birthDate, onBirthDateChange) },
                onBirthDateChange,
                onSexChange,
                onSave,
                onDismiss,
            ),
        )
    }
}

internal data class ChildProfileUiState(
    val editing: Boolean,
    val name: String,
    val birthDate: String,
    val sex: UByte,
    val saving: Boolean,
    val canClearBirthDate: Boolean,
    val ageLabel: String,
)

internal data class ChildProfileActions(
    val onNameChange: (String) -> Unit = {},
    val onChooseBirthDate: () -> Unit = {},
    val onBirthDateChange: (String) -> Unit = {},
    val onSexChange: (UByte) -> Unit = {},
    val onSave: () -> Unit = {},
    val onDismiss: () -> Unit = {},
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ChildProfileContent(
    state: ChildProfileUiState,
    actions: ChildProfileActions,
    scrollState: androidx.compose.foundation.ScrollState = rememberScrollState(),
) {
    val context = LocalContext.current
    val editing = state.editing
    val name = state.name
    val birthDate = state.birthDate
    val sex = state.sex
    val saving = state.saving
    val canClearBirthDate = state.canClearBirthDate
    val ageLabel = state.ageLabel
    val onNameChange = actions.onNameChange
    val onChooseBirthDate = actions.onChooseBirthDate
    val onBirthDateChange = actions.onBirthDateChange
    val onSexChange = actions.onSexChange
    val onSave = actions.onSave
    val onDismiss = actions.onDismiss
    Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
        Scaffold(
            topBar = {
                TopAppBar(
                    title = {
                        Text(
                            stringResource(
                                if (editing) R.string.edit_child_profile
                                else R.string.create_child_profile
                            )
                        )
                    },
                    navigationIcon = {
                        IconButton(onClick = onDismiss) {
                            Icon(
                                painterResource(R.drawable.ic_back),
                                contentDescription = stringResource(R.string.back),
                            )
                        }
                    },
                )
            },
            bottomBar = {
                Surface(Modifier.navigationBarsPadding(), shadowElevation = 8.dp) {
                    Column {
                        Button(
                            onClick = onSave,
                            enabled = name.isNotBlank() && !saving,
                            modifier = Modifier.fillMaxWidth().padding(16.dp).heightIn(min = 56.dp),
                        ) {
                            Text(
                                stringResource(
                                    if (editing) R.string.save_changes else R.string.add_child
                                )
                            )
                        }
                        Spacer(Modifier.height(40.dp))
                    }
                }
            },
        ) { padding: PaddingValues ->
            Column(
                modifier =
                    Modifier.fillMaxSize()
                        .padding(padding)
                        .verticalScroll(scrollState)
                        .padding(horizontal = 20.dp, vertical = 24.dp),
                verticalArrangement = Arrangement.spacedBy(20.dp),
            ) {
                Box(
                    modifier =
                        Modifier.align(Alignment.CenterHorizontally)
                            .size(88.dp)
                            .background(MaterialTheme.colorScheme.primaryContainer, CircleShape),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        name.trim().firstOrNull()?.uppercase() ?: "?",
                        style = MaterialTheme.typography.headlineLarge,
                        color = MaterialTheme.colorScheme.onPrimaryContainer,
                    )
                }
                Text(
                    ageLabel,
                    modifier = Modifier.align(Alignment.CenterHorizontally),
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                HorizontalDivider()
                OutlinedTextField(
                    value = name,
                    onValueChange = { onNameChange(it.take(16 * 1024)) },
                    label = { Text(stringResource(R.string.child_name)) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(stringResource(R.string.birth_date), fontWeight = FontWeight.SemiBold)
                    OutlinedButton(
                        onClick = onChooseBirthDate,
                        modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp),
                    ) {
                        Text(birthDateLabel(context, birthDate))
                    }
                    if (birthDate.isNotBlank() && canClearBirthDate)
                        TextButton(onClick = { onBirthDateChange("") }) {
                            Text(stringResource(R.string.clear_birth_date))
                        }
                }
                HorizontalDivider()
                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(
                        stringResource(R.string.growth_chart_sex),
                        fontWeight = FontWeight.SemiBold,
                    )
                    listOf(
                            1u.toUByte() to R.string.sex_female,
                            2u.toUByte() to R.string.sex_male,
                            3u.toUByte() to R.string.sex_unspecified,
                        )
                        .forEach { (code, label) ->
                            FilterChip(
                                selected = sex == code,
                                onClick = { onSexChange(code) },
                                label = { Text(stringResource(label)) },
                                modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp),
                            )
                        }
                }
            }
        }
    }
}
