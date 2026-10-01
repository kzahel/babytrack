package org.babytrack.app

import android.content.Context
import uniffi.babytrack_core_ffi.ActivityRow

/** One-line text for a projected entry. Values come from the core's read model. */
internal fun entrySummary(context: Context, entry: ActivityRow): String =
    with(context) {
        when {
            entry.bottleMl != null ->
                getString(
                    R.string.bottle_with_entered,
                    localizedEntered(context, entry.bottleEntered ?: entry.bottleMl.toString()),
                    getString(
                        when (entry.bottleUnit) {
                            2u.toUByte() -> R.string.unit_us_fl_oz
                            3u.toUByte() -> R.string.unit_uk_fl_oz
                            else -> R.string.unit_ml
                        }
                    ),
                    getString(
                        when (entry.bottleContent) {
                            1u.toUByte() -> R.string.bottle_breast_milk
                            2u.toUByte() -> R.string.bottle_formula
                            3u.toUByte() -> R.string.bottle_mixed
                            else -> R.string.bottle_other
                        }
                    ),
                )
            entry.kind == "feed.breast" && entry.breastSide != null && entry.endUtcMs != null ->
                getString(
                    R.string.breast_entry,
                    getString(
                        if (entry.breastSide == 1u.toUByte()) R.string.breast_left
                        else R.string.breast_right
                    ),
                    (entry.endUtcMs!! - entry.startUtcMs) / 60_000L,
                )
            entry.kind == "feed.breast" && entry.breastSegments != null ->
                getString(
                    R.string.breast_multi_entry,
                    entry.breastSegments!!.joinToString(" → ") { segment ->
                        getString(
                            R.string.breast_segment_summary,
                            getString(
                                if (segment.side == 1u.toUByte()) R.string.breast_left
                                else R.string.breast_right
                            ),
                            (segment.endUtcMs - segment.startUtcMs) / 60_000L,
                        )
                    },
                )
            entry.kind == "pump" && entry.pumpTotalMl != null && entry.endUtcMs != null ->
                getString(
                    R.string.pump_total_entry,
                    entry.pumpTotalMl!!,
                    (entry.endUtcMs!! - entry.startUtcMs) / 60_000L,
                )
            entry.kind == "pump" &&
                entry.endUtcMs != null &&
                (entry.pumpLeftMl != null || entry.pumpRightMl != null) ->
                getString(
                    R.string.pump_sides_entry,
                    entry.pumpLeftMl ?: 0L,
                    entry.pumpRightMl ?: 0L,
                    (entry.endUtcMs!! - entry.startUtcMs) / 60_000L,
                )
            entry.kind == "feed.solids" && entry.solidsFoods != null -> {
                val foods = entry.solidsFoods!!.joinToString(", ")
                if (entry.solidsAmount.isNullOrBlank()) getString(R.string.solids_entry, foods)
                else getString(R.string.solids_entry_amount, foods, entry.solidsAmount!!)
            }
            entry.kind == "sleep" && entry.endUtcMs != null ->
                getString(R.string.sleep_duration, (entry.endUtcMs!! - entry.startUtcMs) / 60_000)
            entry.kind == "sleep" -> getString(R.string.sleep_running)
            entry.kind == "note" && entry.note != null ->
                getString(R.string.note_entry, entry.note!!)
            entry.kind == "growth" -> {
                val parts =
                    listOfNotNull(
                        growthDisplay(
                            context,
                            entry.growthWeightG,
                            entry.growthWeightEntered,
                            entry.growthWeightUnit,
                            10u.toUByte(),
                        ),
                        growthDisplay(
                            context,
                            entry.growthLengthMm,
                            entry.growthLengthEntered,
                            entry.growthLengthUnit,
                            20u.toUByte(),
                        ),
                        growthDisplay(
                                context,
                                entry.growthHeadMm,
                                entry.growthHeadEntered,
                                entry.growthHeadUnit,
                                20u.toUByte(),
                            )
                            ?.let { getString(R.string.growth_head_part, it) },
                    )
                if (parts.isEmpty()) entry.kind
                else getString(R.string.growth_summary, parts.joinToString(" · "))
            }
            entry.kind == "temperature" && entry.temperatureC != null ->
                getString(
                    R.string.temperature_entry,
                    localizedEntered(context, entry.temperatureEntered ?: entry.temperatureC!!),
                    getString(
                        if (entry.temperatureUnit == 31u.toUByte()) R.string.unit_fahrenheit
                        else R.string.unit_celsius
                    ),
                )
            entry.kind == "medication" &&
                entry.medicationName != null &&
                entry.medicationDoseAmount != null &&
                entry.medicationDoseUnit != null ->
                getString(
                    R.string.medication_entry,
                    entry.medicationName!!,
                    entry.medicationDoseAmount!!,
                    entry.medicationDoseUnit!!,
                )
            entry.diaperKind != null ->
                getString(
                    R.string.diaper,
                    getString(
                        when (entry.diaperKind!!.toInt()) {
                            1 -> R.string.wet
                            2 -> R.string.dirty
                            3 -> R.string.both
                            else -> R.string.dry
                        }
                    ),
                )
            else -> getString(R.string.unknown_activity)
        }
    }

internal fun sleepPlaceLabel(place: UByte): Int =
    when (place.toInt()) {
        1 -> R.string.sleep_place_crib
        2 -> R.string.sleep_place_pram
        3 -> R.string.sleep_place_contact
        4 -> R.string.sleep_place_car
        else -> R.string.sleep_place_other
    }
