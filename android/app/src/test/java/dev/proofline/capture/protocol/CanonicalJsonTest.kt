package dev.proofline.capture.protocol

import dev.proofline.capture.security.Crypto
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Test

class CanonicalJsonTest {
    @Test fun sharedFragmentVectorMatches() {
        val text = requireNotNull(javaClass.classLoader?.getResource("fragment-chain-v2.json")).readText()
        val fixture = CanonicalJson.json.parseToJsonElement(text).jsonObject
        val canonical = CanonicalJson.encode(fixture.getValue("chain_input"))
        assertEquals(fixture.getValue("canonical").jsonPrimitive.content, canonical)
        assertEquals(fixture.getValue("chain_digest").jsonPrimitive.content, Crypto.sha256Hex(canonical.toByteArray()))
    }
}
