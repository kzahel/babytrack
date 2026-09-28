package org.babytrack.app

/** The Rust core stores protocol decimals with an ASCII dot. */
internal fun canonicalDecimal(value: String): String = value.trim().replace(',', '.')

internal fun decimalDraft(value: String, fractional: Boolean, signed: Boolean = false): String =
    value.filterIndexed { index, char ->
        char in '0'..'9' || (fractional && (char == '.' || char == ',')) ||
            (signed && index == 0 && char == '-')
    }.take(16)

internal fun localizedDecimal(value: String, separator: Char): String =
    if (separator == '.') value else value.replace('.', separator)
