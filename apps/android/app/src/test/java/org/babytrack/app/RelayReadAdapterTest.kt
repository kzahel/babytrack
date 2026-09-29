package org.babytrack.app

import java.net.UnknownHostException
import org.junit.Assert.assertSame
import org.junit.Assert.assertThrows
import org.junit.Test
import uniffi.babytrack_core_ffi.BindingException

class RelayReadAdapterTest {
    @Test fun offlineAndHttpFailuresUseTheDeclaredCallbackError() {
        for (failure in listOf(UnknownHostException("offline"), IllegalStateException("HTTP 503"))) {
            val reads = relayReads { _, _ -> throw failure }
            val declared = assertThrows(BindingException.Rejected::class.java) {
                reads.get("/v1/families/test/log", byteArrayOf(1))
            }
            assertSame(failure, declared.cause)
        }
    }

    @Test fun alreadyDeclaredErrorsKeepTheirIdentity() {
        val failure = BindingException.InvalidBytes()
        val reads = relayReads { _, _ -> throw failure }
        assertSame(failure, assertThrows(BindingException.InvalidBytes::class.java) {
            reads.get("/v1/families/test/log", byteArrayOf(1))
        })
    }
}
