package org.babytrack.app

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertThrows
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.InputStream

class BackupInputTest {
    @Test fun acceptsExactLimit() {
        val bytes = ByteArray(17_000) { it.toByte() }
        assertArrayEquals(bytes, readBounded(ByteArrayInputStream(bytes), bytes.size.toLong()))
    }

    @Test fun rejectsUnknownLengthProviderWithoutReadingWholeStream() {
        var reads = 0
        val provider = object : InputStream() {
            override fun read(): Int { reads++; return 42 }
            override fun read(buffer: ByteArray, offset: Int, length: Int): Int {
                reads++
                buffer.fill(42, offset, offset + length)
                return length
            }
        }
        assertThrows(BackupTooLarge::class.java) { readBounded(provider, 16_384) }
        if (reads > 3) throw AssertionError("read continued after limit: $reads")
    }
}
