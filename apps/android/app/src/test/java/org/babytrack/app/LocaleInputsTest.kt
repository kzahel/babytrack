package org.babytrack.app

import org.junit.Assert.assertEquals
import org.junit.Test

class LocaleInputsTest {
    @Test fun commaDecimalIsCanonicalOnlyAtTheCoreBoundary() {
        assertEquals("4.25", canonicalDecimal(" 4,25 "))
        assertEquals("4,25", decimalDraft("4,25", fractional = true))
        assertEquals("425", decimalDraft("4,25", fractional = false))
        assertEquals("-37,5", decimalDraft("-37,5", fractional = true, signed = true))
        assertEquals("4,25", localizedDecimal("4.25", ','))
    }
}
