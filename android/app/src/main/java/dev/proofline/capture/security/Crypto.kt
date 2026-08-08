package dev.proofline.capture.security

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.security.keystore.KeyInfo
import android.security.keystore.StrongBoxUnavailableException
import android.os.Build
import android.util.Base64
import java.security.KeyPair
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.MessageDigest
import java.security.Signature
import java.security.spec.ECGenParameterSpec

data class DeviceIdentity(
    val publicKeySpki: String,
    val fingerprint: String,
    val certificateChain: List<String>,
    val requestedAssurance: String,
)

object Crypto {
    private const val DEVICE_ALIAS = "proofline-device-identity-v2"

    fun hasDeviceIdentity(): Boolean = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }.containsAlias(DEVICE_ALIAS)

    fun ensureDeviceIdentity(attestationChallenge: ByteArray): DeviceIdentity {
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        if (!keyStore.containsAlias(DEVICE_ALIAS)) {
            try { generateDeviceKey(attestationChallenge, true) }
            catch (_: StrongBoxUnavailableException) { generateDeviceKey(attestationChallenge, false) }
        }
        val entry = keyStore.getEntry(DEVICE_ALIAS, null) as KeyStore.PrivateKeyEntry
        val spki = base64Url(entry.certificate.publicKey.encoded)
        val security: KeyInfo? = runCatching {
            val factory = java.security.KeyFactory.getInstance(entry.privateKey.algorithm, "AndroidKeyStore")
            factory.getKeySpec(entry.privateKey, android.security.keystore.KeyInfo::class.java)
        }.getOrNull()
        val assurance = security?.let(::localAssurance) ?: "software_attested"
        return DeviceIdentity(spki, base32(MessageDigest.getInstance("SHA-256").digest(entry.certificate.publicKey.encoded)), entry.certificateChain.map { base64Url(it.encoded) }, assurance)
    }

    @Suppress("DEPRECATION") // API 30 exposes only this boolean; API 31+ uses securityLevel below.
    private fun localAssurance(info: KeyInfo): String = if (Build.VERSION.SDK_INT >= 31) {
        when (info.securityLevel) {
            KeyProperties.SECURITY_LEVEL_STRONGBOX -> "strongbox"
            KeyProperties.SECURITY_LEVEL_TRUSTED_ENVIRONMENT -> "tee"
            else -> "software_attested"
        }
    } else if (info.isInsideSecureHardware) "tee" else "software_attested"

    private fun generateDeviceKey(challenge: ByteArray, strongBox: Boolean) {
        val spec = KeyGenParameterSpec.Builder(DEVICE_ALIAS, KeyProperties.PURPOSE_SIGN)
            .setAlgorithmParameterSpec(ECGenParameterSpec("secp256r1"))
            .setDigests(KeyProperties.DIGEST_SHA256)
            .setAttestationChallenge(challenge)
            .setUserAuthenticationRequired(false)
            .setIsStrongBoxBacked(strongBox)
            .build()
        KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC, "AndroidKeyStore").apply { initialize(spec) }.generateKeyPair()
    }

    fun newSessionKey(): KeyPair = KeyPairGenerator.getInstance("EC").apply { initialize(ECGenParameterSpec("secp256r1")) }.generateKeyPair()
    fun sessionSpki(pair: KeyPair): String = base64Url(pair.public.encoded)

    fun signDevice(canonical: String): String {
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        val privateKey = (keyStore.getEntry(DEVICE_ALIAS, null) as KeyStore.PrivateKeyEntry).privateKey
        return sign(privateKey, canonical)
    }

    fun signSession(pair: KeyPair, canonical: String): String = sign(pair.private, canonical)

    private fun sign(key: java.security.PrivateKey, canonical: String): String {
        val signature = Signature.getInstance("SHA256withECDSA").apply { initSign(key); update(canonical.toByteArray()) }.sign()
        return base64Url(derToP1363(signature))
    }

    fun sha256Hex(bytes: ByteArray): String = MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }
    fun base64Url(bytes: ByteArray): String = Base64.encodeToString(bytes, Base64.URL_SAFE or Base64.NO_WRAP or Base64.NO_PADDING)
    fun decodeBase64Url(value: String): ByteArray = Base64.decode(value, Base64.URL_SAFE or Base64.NO_WRAP or Base64.NO_PADDING)

    private fun derToP1363(der: ByteArray): ByteArray {
        require(der.size >= 8 && der[0] == 0x30.toByte()) { "Invalid ECDSA DER signature" }
        var offset = 2
        if ((der[1].toInt() and 0x80) != 0) offset = 2 + (der[1].toInt() and 0x7f)
        require(der[offset] == 0x02.toByte())
        val rLength = der[offset + 1].toInt() and 0xff
        val r = der.copyOfRange(offset + 2, offset + 2 + rLength)
        offset += 2 + rLength
        require(der[offset] == 0x02.toByte())
        val sLength = der[offset + 1].toInt() and 0xff
        val s = der.copyOfRange(offset + 2, offset + 2 + sLength)
        fun fixed(value: ByteArray): ByteArray {
            val unsigned = value.dropWhile { it == 0.toByte() }.toByteArray()
            require(unsigned.size <= 32)
            return ByteArray(32 - unsigned.size) + unsigned
        }
        return fixed(r) + fixed(s)
    }

    private fun base32(bytes: ByteArray): String {
        val alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"
        var buffer = 0; var bits = 0
        return buildString {
            bytes.forEach { byte ->
                buffer = (buffer shl 8) or (byte.toInt() and 0xff); bits += 8
                while (bits >= 5) { append(alphabet[(buffer shr (bits - 5)) and 31]); bits -= 5 }
            }
            if (bits > 0) append(alphabet[(buffer shl (5 - bits)) and 31])
        }.lowercase()
    }
}
