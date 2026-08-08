package dev.proofline.capture.protocol

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement

@Serializable data class ChallengeRequest(val phase: String = "challenge")
@Serializable data class ChallengeResponse(val challenge: String, val expiresAt: String)
@Serializable data class AttestationRequest(
    val phase: String = "verify",
    val publicKeySpki: String,
    val certificateChain: List<String>? = null,
    val challenge: String,
    val claimedAssurance: String? = null,
)
@Serializable data class AttestationResponse(val fingerprint: String, val assuranceLevel: String)
@Serializable data class StreamDeclaration(
    val id: String, val role: String, val mimeType: String = "video/mp4", val codec: String = "avc1.640028",
    val width: Int? = null, val height: Int? = null, val fps: Double? = 30.0, val hasAudio: Boolean = false,
)
@Serializable data class GeoPoint(val latitude: Double, val longitude: Double, val accuracyM: Float)
@Serializable data class CreateCaptureRequest(
    val sessionNonce: String, val deviceFingerprint: String, val assuranceLevel: String, val devicePublicKeySpki: String,
    val sessionPublicKeySpki: String, val sessionBindingSignature: String, val title: String,
    val startedAt: String, val streams: List<StreamDeclaration>, val location: GeoPoint? = null,
)
@Serializable data class CreateCaptureResponse(
    val captureId: String, val uploadToken: String, val mediaBaseUrl: String, val expiresAt: String,
    val maxDurationSeconds: Int, val fragmentDurationMs: Long,
)
@Serializable data class SessionBinding(
    val protocolVersion: String = "proofline/2", val challenge: String, val deviceFingerprint: String,
    val sessionPublicKeySpki: String, val startedAt: String, val streams: List<StreamDeclaration>,
)
@Serializable data class FragmentEnvelope(
    @SerialName("protocol_version") val protocolVersion: String = "proofline/2",
    @SerialName("capture_id") val captureId: String,
    @SerialName("stream_id") val streamId: String,
    val sequence: Long,
    @SerialName("previous_chain_digest") val previousChainDigest: String,
    @SerialName("media_digest") val mediaDigest: String,
    @SerialName("chain_digest") val chainDigest: String,
    @SerialName("byte_length") val byteLength: Long,
    @SerialName("pts_start_us") val ptsStartUs: Long,
    @SerialName("pts_end_us") val ptsEndUs: Long,
    @SerialName("telemetry_root") val telemetryRoot: String,
)
@Serializable data class FragmentChainInput(
    @SerialName("protocol_version") val protocolVersion: String = "proofline/2",
    @SerialName("capture_id") val captureId: String,
    @SerialName("stream_id") val streamId: String,
    val sequence: Long,
    @SerialName("previous_chain_digest") val previousChainDigest: String,
    @SerialName("media_digest") val mediaDigest: String,
    @SerialName("byte_length") val byteLength: Long,
    @SerialName("pts_start_us") val ptsStartUs: Long,
    @SerialName("pts_end_us") val ptsEndUs: Long,
    @SerialName("telemetry_root") val telemetryRoot: String,
)
@Serializable data class EndStream(val id: String, val sequenceCount: Long, val finalChainDigest: String)
@Serializable data class EndManifest(
    val protocolVersion: String = "proofline/2", val captureId: String, val endedAt: String,
    val durationMs: Long, val closeReason: String, val streams: List<EndStream>,
)
@Serializable data class EndRequest(val manifest: EndManifest, val signature: String)
@Serializable data class TelemetryBatch(
    val captureId: String, val sequence: Long, val previousDigest: String, val digest: String,
    val signature: String, val samples: JsonElement,
)

const val GENESIS_DIGEST = "0000000000000000000000000000000000000000000000000000000000000000"
