package org.babytrack.app

import android.content.Context
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import java.text.DecimalFormatSymbols
import java.time.LocalDate
import uniffi.babytrack_core_ffi.ChildRow
import uniffi.babytrack_core_ffi.EnteredMeasureRow
import uniffi.babytrack_core_ffi.GrowthInputRow

internal val bottleAmountPattern = Regex("(0|[1-9][0-9]*)(\\.[0-9]+)?")
internal val temperaturePattern = Regex("-?(0|[1-9][0-9]*)(\\.[0-9]+)?")

internal fun validBottleAmount(value: String, unit: UByte): Boolean =
    value.length <= 16 &&
        bottleAmountPattern.matches(canonicalDecimal(value)) &&
        (unit != 1u.toUByte() || !canonicalDecimal(value).contains('.')) &&
        value.any { it in '1'..'9' } &&
        (unit != 1u.toUByte() ||
            canonicalDecimal(value).toLongOrNull()?.let { it in 1..1_000_000 } == true)

internal fun validGrowthAmount(value: String, unit: UByte): Boolean =
    value.isBlank() ||
        (value.length <= 16 &&
            bottleAmountPattern.matches(canonicalDecimal(value)) &&
            value.any { it in '1'..'9' } &&
            (unit !in listOf(10u.toUByte(), 20u.toUByte()) ||
                !canonicalDecimal(value).contains('.')))

internal fun validTemperature(value: String): Boolean =
    value.length <= 16 && temperaturePattern.matches(canonicalDecimal(value))

internal fun growthInput(
    weight: String,
    weightUnit: UByte,
    length: String,
    lengthUnit: UByte,
    head: String,
    headUnit: UByte,
): GrowthInputRow =
    GrowthInputRow(
        weight
            .trim()
            .takeIf { it.isNotEmpty() }
            ?.let { EnteredMeasureRow(canonicalDecimal(it), weightUnit) },
        length
            .trim()
            .takeIf { it.isNotEmpty() }
            ?.let { EnteredMeasureRow(canonicalDecimal(it), lengthUnit) },
        head
            .trim()
            .takeIf { it.isNotEmpty() }
            ?.let { EnteredMeasureRow(canonicalDecimal(it), headUnit) },
    )

internal fun localizedEntered(context: Context, value: String): String =
    localizedDecimal(
        value,
        DecimalFormatSymbols.getInstance(context.resources.configuration.locales[0])
            .decimalSeparator,
    )

internal fun ChildRow.birthDateString(): String =
    birthDay
        ?.let { day -> runCatching { LocalDate.ofEpochDay(day).toString() }.getOrNull() }
        .orEmpty()

internal fun growthUnitLabel(context: Context, unit: UByte): String =
    context.getString(
        when (unit) {
            10u.toUByte() -> R.string.unit_g
            11u.toUByte() -> R.string.unit_kg
            12u.toUByte() -> R.string.unit_lb
            13u.toUByte() -> R.string.unit_oz_mass
            20u.toUByte() -> R.string.unit_mm
            21u.toUByte() -> R.string.unit_cm
            else -> R.string.unit_in
        }
    )

internal fun growthDisplay(
    context: Context,
    base: Long?,
    entered: String?,
    unit: UByte?,
    baseUnit: UByte,
): String? =
    base?.let {
        val known =
            when (baseUnit) {
                10u.toUByte() -> massUnits.any { it.first == unit }
                else -> lengthUnits.any { it.first == unit }
            }
        val shownUnit = if (known) unit!! else baseUnit
        "${localizedEntered(context, if (known) entered ?: it.toString() else it.toString())} ${growthUnitLabel(context, shownUnit)}"
    }

@Composable
internal fun GrowthUnitChoices(
    title: Int,
    units: List<Pair<UByte, Int>>,
    selected: UByte,
    onSelect: (UByte) -> Unit,
) {
    Text(
        stringResource(title),
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
    SegmentedChoice(units.map { (unit, label) -> unit to stringResource(label) }, selected, onSelect)
}

internal val massUnits =
    listOf(
        10u.toUByte() to R.string.unit_g,
        11u.toUByte() to R.string.unit_kg,
        12u.toUByte() to R.string.unit_lb,
        13u.toUByte() to R.string.unit_oz_mass,
    )
internal val lengthUnits =
    listOf(
        20u.toUByte() to R.string.unit_mm,
        21u.toUByte() to R.string.unit_cm,
        22u.toUByte() to R.string.unit_in,
    )
