package org.babytrack.app

import java.io.ByteArrayOutputStream
import java.io.InputStream

internal class BackupTooLarge : IllegalArgumentException()

// UniFFI copies the byte array once into Rust. Leave room for parsing,
// projection, and the rest of the app rather than trusting a provider's size.
internal fun backupReadLimit(availableMemoryBytes: Long): Long =
    minOf(128L * 1024 * 1024, availableMemoryBytes.coerceAtLeast(0) / 8)

internal fun readBounded(input: InputStream, maxBytes: Long): ByteArray {
    if (maxBytes <= 0) throw BackupTooLarge()
    val output = ByteArrayOutputStream()
    val chunk = ByteArray(8 * 1024)
    var total = 0L
    while (true) {
        val count = input.read(chunk)
        if (count < 0) break
        if (count == 0) continue
        total += count
        if (total > maxBytes) throw BackupTooLarge()
        output.write(chunk, 0, count)
    }
    return output.toByteArray()
}
