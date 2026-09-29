package org.babytrack.app

import uniffi.babytrack_core_ffi.BindingException
import uniffi.babytrack_core_ffi.RelayReadTransport

/** Return the declared callback error to Rust instead of an unexpected foreign exception. */
internal fun relayReads(read: (String, ByteArray) -> ByteArray): RelayReadTransport =
    object : RelayReadTransport {
        override fun get(path: String, auth: ByteArray): ByteArray = try {
            read(path, auth)
        } catch (failure: BindingException) {
            throw failure
        } catch (failure: Exception) {
            // UniFFI treats other exception types as unexpected callback failures.
            // Those can panic while Rust owns the store mutex and poison later local reads.
            throw BindingException.Rejected("Relay read unavailable").also { it.initCause(failure) }
        }
    }
