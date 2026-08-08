package dev.proofline.capture.capture

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import androidx.annotation.RequiresPermission
import androidx.camera.core.CameraSelector
import androidx.camera.core.ConcurrentCamera
import androidx.camera.core.UseCaseGroup
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.video.FileOutputOptions
import androidx.camera.video.Quality
import androidx.camera.video.QualitySelector
import androidx.camera.video.Recorder
import androidx.camera.video.Recording
import androidx.camera.video.VideoCapture
import androidx.camera.video.VideoRecordEvent
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleOwner
import dev.proofline.capture.protocol.CanonicalJson
import dev.proofline.capture.protocol.CreateCaptureResponse
import dev.proofline.capture.protocol.FragmentChainInput
import dev.proofline.capture.protocol.FragmentEnvelope
import dev.proofline.capture.protocol.GENESIS_DIGEST
import dev.proofline.capture.protocol.StreamDeclaration
import dev.proofline.capture.protocol.TelemetryBatch
import dev.proofline.capture.security.Crypto
import dev.proofline.capture.storage.EncryptedFragmentQueue
import dev.proofline.capture.telemetry.TelemetryCollector
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.serialization.json.encodeToJsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import java.io.File
import java.security.KeyPair
import java.util.UUID
import java.util.concurrent.Executor
import java.util.concurrent.atomic.AtomicInteger
import kotlin.coroutines.resume

private data class StreamSlot(
    val declaration: StreamDeclaration,
    val recorder: Recorder,
    val capture: VideoCapture<Recorder>,
    val audio: Boolean,
    var sequence: Long = 0,
    var previousDigest: String = GENESIS_DIGEST,
    var recording: Recording? = null,
)

class CaptureEngine(
    private val context: Context,
    private val lifecycleOwner: LifecycleOwner,
    private val scope: CoroutineScope,
    private val sessionKey: KeyPair,
    private val session: CreateCaptureResponse,
    private val queue: EncryptedFragmentQueue,
    private val telemetry: TelemetryCollector,
    declarations: List<StreamDeclaration>,
) {
    private val executor: Executor = ContextCompat.getMainExecutor(context)
    private val inFlight = AtomicInteger(0)
    private val slots = declarations.map { declaration ->
        val recorder = Recorder.Builder()
            .setQualitySelector(QualitySelector.fromOrderedList(listOf(Quality.FHD, Quality.HD, Quality.SD)))
            .build()
        StreamSlot(declaration, recorder, VideoCapture.withOutput(recorder), declaration.hasAudio)
    }
    private var active = false
    private var telemetrySequence = 0L
    private var previousTelemetryDigest = GENESIS_DIGEST
    private lateinit var provider: ProcessCameraProvider

    @RequiresPermission(allOf = [Manifest.permission.CAMERA, Manifest.permission.RECORD_AUDIO])
    suspend fun start(): Int {
        provider = awaitCameraProvider(context)
        provider.unbindAll()
        active = true
        var bound = 1
        val rear = slots.first()
        val front = slots.getOrNull(1)
        if (front != null) {
            try {
                val rearConfig = ConcurrentCamera.SingleCameraConfig(CameraSelector.DEFAULT_BACK_CAMERA, UseCaseGroup.Builder().addUseCase(rear.capture).build(), lifecycleOwner)
                val frontConfig = ConcurrentCamera.SingleCameraConfig(CameraSelector.DEFAULT_FRONT_CAMERA, UseCaseGroup.Builder().addUseCase(front.capture).build(), lifecycleOwner)
                provider.bindToLifecycle(listOf(rearConfig, frontConfig))
                bound = 2
            } catch (error: Throwable) {
                // A device can advertise concurrent cameras but reject the exact pair/use-case
                // combination. The absent front stream remains declared with a zero-fragment end.
                provider.unbindAll()
                provider.bindToLifecycle(lifecycleOwner, CameraSelector.DEFAULT_BACK_CAMERA, rear.capture)
                CaptureStatusStore.update { it.copy(message = "Concurrent cameras were advertised but rejected; rear capture continues (${error.javaClass.simpleName})") }
            }
        } else provider.bindToLifecycle(lifecycleOwner, CameraSelector.DEFAULT_BACK_CAMERA, rear.capture)
        slots.take(bound).forEach(::startSegment)
        return bound
    }

    @RequiresPermission(Manifest.permission.RECORD_AUDIO)
    private fun startSegment(slot: StreamSlot) {
        if (!active) return
        val file = File(context.noBackupFilesDir, "segment-${session.captureId}-${slot.declaration.id}-${slot.sequence}-${UUID.randomUUID()}.mp4")
        val options = FileOutputOptions.Builder(file).setDurationLimitMillis(session.fragmentDurationMs).build()
        var pending = slot.recorder.prepareRecording(context, options)
        if (slot.audio) pending = pending.withAudioEnabled()
        slot.recording = pending.start(executor) { event ->
            if (event is VideoRecordEvent.Finalize) {
                slot.recording = null
                inFlight.incrementAndGet()
                scope.launch(Dispatchers.IO) {
                    try { processFinalized(slot, file, event.recordingStats.recordedDurationNanos / 1_000) }
                    catch (error: Throwable) {
                        CaptureStatusStore.update { it.copy(message = "Fragment processing failed: ${error.message}", fatal = true) }
                        active = false
                    } finally {
                        inFlight.decrementAndGet()
                        file.delete()
                        if (active) scope.launch(Dispatchers.Main) { startSegment(slot) }
                    }
                }
            }
        }
    }

    private fun processFinalized(slot: StreamSlot, file: File, durationUs: Long) {
        val bytes = file.readBytes()
        if (bytes.size < 32 || !containsBox(bytes, "ftyp")) throw IllegalStateException("Camera output is not an ISO BMFF asset")
        val sequence = slot.sequence
        val mediaDigest = Crypto.sha256Hex(bytes)
        val ptsStartUs = sequence * session.fragmentDurationMs * 1_000
        val chainInput = FragmentChainInput(
            captureId = session.captureId, streamId = slot.declaration.id, sequence = sequence,
            previousChainDigest = slot.previousDigest, mediaDigest = mediaDigest, byteLength = bytes.size.toLong(),
            ptsStartUs = ptsStartUs, ptsEndUs = ptsStartUs + durationUs.coerceAtLeast(1), telemetryRoot = drainTelemetry(),
        )
        val chainDigest = Crypto.sha256Hex(CanonicalJson.encode(CanonicalJson.json.encodeToJsonElement(chainInput)).toByteArray())
        val envelope = FragmentEnvelope(
            captureId = chainInput.captureId, streamId = chainInput.streamId, sequence = sequence,
            previousChainDigest = chainInput.previousChainDigest, mediaDigest = mediaDigest, chainDigest = chainDigest,
            byteLength = chainInput.byteLength, ptsStartUs = chainInput.ptsStartUs, ptsEndUs = chainInput.ptsEndUs,
            telemetryRoot = chainInput.telemetryRoot,
        )
        val signature = Crypto.signSession(sessionKey, CanonicalJson.encode(CanonicalJson.json.encodeToJsonElement(envelope)))
        queue.enqueue(session, envelope, signature, bytes)
        slot.sequence += 1; slot.previousDigest = chainDigest
        CaptureStatusStore.update { it.copy(queued = queue.pendingCount(), message = "Fragment $sequence signed and encrypted locally") }
    }

    @Synchronized private fun drainTelemetry(): String {
        val drained = telemetry.drainBatch()
        if (drained.samples.isEmpty()) return previousTelemetryDigest
        val unsigned = JsonObject(mapOf(
            "captureId" to JsonPrimitive(session.captureId), "sequence" to JsonPrimitive(telemetrySequence),
            "previousDigest" to JsonPrimitive(previousTelemetryDigest), "samples" to drained.samples,
        ))
        val digest = Crypto.sha256Hex(CanonicalJson.encode(unsigned).toByteArray())
        val signed = JsonObject(unsigned + ("digest" to JsonPrimitive(digest)))
        val signature = Crypto.signSession(sessionKey, CanonicalJson.encode(signed))
        queue.enqueueTelemetry(session, TelemetryBatch(session.captureId, telemetrySequence, previousTelemetryDigest, digest, signature, drained.samples))
        telemetrySequence += 1
        previousTelemetryDigest = digest
        return digest
    }

    suspend fun stop(): List<dev.proofline.capture.protocol.EndStream> {
        active = false
        slots.forEach { it.recording?.stop() }
        val deadline = System.currentTimeMillis() + 10_000
        while ((inFlight.get() > 0 || slots.any { it.recording != null }) && System.currentTimeMillis() < deadline) delay(100)
        if (::provider.isInitialized) provider.unbindAll()
        return slots.map { dev.proofline.capture.protocol.EndStream(it.declaration.id, it.sequence, it.previousDigest) }
    }

    private fun containsBox(bytes: ByteArray, type: String): Boolean {
        val needle = type.toByteArray()
        val limit = minOf(bytes.size - needle.size, 64)
        for (offset in 0..limit) if (needle.indices.all { bytes[offset + it] == needle[it] }) return true
        return false
    }

    companion object {
        suspend fun awaitCameraProvider(context: Context): ProcessCameraProvider = suspendCancellableCoroutine { continuation ->
            val future = ProcessCameraProvider.getInstance(context)
            future.addListener({ runCatching { future.get() }.onSuccess(continuation::resume).onFailure { continuation.cancel(it) } }, ContextCompat.getMainExecutor(context))
        }

        fun declarations(context: Context): List<StreamDeclaration> {
            val result = mutableListOf(StreamDeclaration("rear-${UUID.randomUUID()}", "rear_video", hasAudio = true, width = 1920, height = 1080))
            if (context.packageManager.hasSystemFeature(PackageManager.FEATURE_CAMERA_CONCURRENT)) result += StreamDeclaration("front-${UUID.randomUUID()}", "front_video", hasAudio = false, width = 1280, height = 720)
            return result
        }
    }
}
