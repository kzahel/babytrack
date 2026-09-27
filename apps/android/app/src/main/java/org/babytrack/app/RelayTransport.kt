package org.babytrack.app

import java.net.HttpURLConnection
import java.net.URI
import java.net.URL

/** Byte transport only; the Rust core constructs and verifies protocol data. */
internal class RelayTransport(origin: String) {
    private val base: String

    init {
        val parsed = URI(origin)
        require(parsed.rawPath.isNullOrEmpty() && parsed.rawQuery == null && parsed.rawFragment == null)
        require(parsed.rawUserInfo == null)
        require(parsed.scheme == "https" || (parsed.scheme == "http" && parsed.host == "localhost"))
        require(parsed.port in -1..65535 && parsed.port != 0)
        base = origin.trimEnd('/')
    }

    fun post(path: String, bytes: ByteArray): ByteArray {
        require(path.startsWith("/v1/families/") && !path.contains("..") && !path.contains('#'))
        val connection = URL(base + path).openConnection() as HttpURLConnection
        try {
            connection.requestMethod = "POST"
            connection.connectTimeout = 10_000
            connection.readTimeout = 15_000
            connection.instanceFollowRedirects = false
            connection.doOutput = true
            connection.setRequestProperty("Content-Type", "application/cbor")
            connection.outputStream.use { it.write(bytes) }
            check(connection.responseCode == HttpURLConnection.HTTP_OK) {
                "Relay rejected request: ${connection.responseCode}"
            }
            connection.inputStream.use { input ->
                return readBounded(input, 1_048_576)
            }
        } finally {
            connection.disconnect()
        }
    }
}
