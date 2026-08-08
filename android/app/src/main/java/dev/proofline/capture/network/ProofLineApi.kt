package dev.proofline.capture.network

import dev.proofline.capture.BuildConfig
import dev.proofline.capture.protocol.AttestationRequest
import dev.proofline.capture.protocol.AttestationResponse
import dev.proofline.capture.protocol.CanonicalJson
import dev.proofline.capture.protocol.ChallengeRequest
import dev.proofline.capture.protocol.ChallengeResponse
import dev.proofline.capture.protocol.CreateCaptureRequest
import dev.proofline.capture.protocol.CreateCaptureResponse
import dev.proofline.capture.protocol.EndRequest
import dev.proofline.capture.protocol.FragmentEnvelope
import dev.proofline.capture.protocol.TelemetryBatch
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.encodeToJsonElement
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.time.Duration

class ProofLineApi(private val client: OkHttpClient = defaultClient()) {
    private val control = BuildConfig.PROOFLINE_CONTROL_URL.trimEnd('/')
    private val json = CanonicalJson.json

    suspend fun challenge(): ChallengeResponse = post("$control/api/v1/devices/attest", json.encodeToString(ChallengeRequest()))

    suspend fun attest(input: AttestationRequest): AttestationResponse = post("$control/api/v1/devices/attest", json.encodeToString(input))

    suspend fun createCapture(input: CreateCaptureRequest): CreateCaptureResponse = post("$control/api/v1/captures", json.encodeToString(input))

    suspend fun uploadFragment(session: CreateCaptureResponse, envelope: FragmentEnvelope, signature: String, bytes: ByteArray): String = withContext(Dispatchers.IO) {
        val canonicalEnvelope = CanonicalJson.encode(json.encodeToJsonElement(envelope))
        val request = Request.Builder()
            .url("${session.mediaBaseUrl.trimEnd('/')}/ingest/v1/${session.captureId}/${envelope.streamId}/${envelope.sequence}")
            .header("Authorization", "Bearer ${session.uploadToken}")
            .header("X-ProofLine-Envelope", dev.proofline.capture.security.Crypto.base64Url(canonicalEnvelope.toByteArray()))
            .header("X-ProofLine-Signature", signature)
            .put(bytes.toRequestBody("video/mp4".toMediaType()))
            .build()
        client.newCall(request).execute().use { response ->
            val body = response.body?.string().orEmpty()
            if (!response.isSuccessful) throw ApiException(response.code, body)
            body
        }
    }

    suspend fun endCapture(session: CreateCaptureResponse, requestBody: EndRequest): String = withContext(Dispatchers.IO) {
        val request = Request.Builder().url("${session.mediaBaseUrl.trimEnd('/')}/ingest/v1/${session.captureId}/end")
            .header("Authorization", "Bearer ${session.uploadToken}")
            .post(json.encodeToString(requestBody).toRequestBody(JSON_MEDIA)).build()
        client.newCall(request).execute().use { response ->
            val body = response.body?.string().orEmpty()
            if (!response.isSuccessful) throw ApiException(response.code, body)
            body
        }
    }

    suspend fun uploadTelemetry(session: CreateCaptureResponse, batch: TelemetryBatch): String = withContext(Dispatchers.IO) {
        val request = Request.Builder().url("${session.mediaBaseUrl.trimEnd('/')}/ingest/v1/${session.captureId}/telemetry")
            .header("Authorization", "Bearer ${session.uploadToken}")
            .post(json.encodeToString(batch).toRequestBody(JSON_MEDIA)).build()
        client.newCall(request).execute().use { response ->
            val body = response.body?.string().orEmpty()
            if (!response.isSuccessful) throw ApiException(response.code, body)
            body
        }
    }

    private suspend inline fun <reified T> post(url: String, body: String): T = withContext(Dispatchers.IO) {
        val request = Request.Builder().url(url).post(body.toRequestBody(JSON_MEDIA)).build()
        client.newCall(request).execute().use { response ->
            val responseBody = response.body?.string().orEmpty()
            if (!response.isSuccessful) throw ApiException(response.code, responseBody)
            json.decodeFromString<T>(responseBody)
        }
    }

    companion object {
        private val JSON_MEDIA = "application/json; charset=utf-8".toMediaType()
        private fun defaultClient() = OkHttpClient.Builder()
            .connectTimeout(Duration.ofSeconds(15)).readTimeout(Duration.ofSeconds(30)).writeTimeout(Duration.ofSeconds(45))
            .retryOnConnectionFailure(true).build()
    }
}

class ApiException(val status: Int, body: String) : java.io.IOException("ProofLine API returned $status: ${body.take(500)}")
