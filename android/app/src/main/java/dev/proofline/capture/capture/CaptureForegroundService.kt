package dev.proofline.capture.capture

import android.Manifest
import android.annotation.SuppressLint
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.PackageManager
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleService
import androidx.lifecycle.lifecycleScope
import dev.proofline.capture.MainActivity
import dev.proofline.capture.network.ProofLineApi
import dev.proofline.capture.protocol.AttestationRequest
import dev.proofline.capture.protocol.CanonicalJson
import dev.proofline.capture.protocol.CreateCaptureRequest
import dev.proofline.capture.protocol.EndManifest
import dev.proofline.capture.protocol.EndRequest
import dev.proofline.capture.protocol.GeoPoint
import dev.proofline.capture.protocol.SessionBinding
import dev.proofline.capture.security.Crypto
import dev.proofline.capture.storage.EncryptedFragmentQueue
import dev.proofline.capture.storage.EvidenceUploadWorker
import dev.proofline.capture.storage.FragmentUploader
import dev.proofline.capture.telemetry.TelemetryCollector
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.serialization.json.encodeToJsonElement
import java.time.Instant
import java.security.KeyPair

class CaptureForegroundService : LifecycleService() {
    private val api = ProofLineApi()
    private lateinit var queue: EncryptedFragmentQueue
    private lateinit var telemetry: TelemetryCollector
    private var engine: CaptureEngine? = null
    private var uploader: Job? = null
    private var captureJob: Job? = null
    private var startedAtMs = 0L
    private var currentSession: dev.proofline.capture.protocol.CreateCaptureResponse? = null
    private var currentSessionKey: KeyPair? = null

    override fun onCreate() {
        super.onCreate()
        queue = EncryptedFragmentQueue(this)
        telemetry = TelemetryCollector(this)
        createChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        super.onStartCommand(intent, flags, startId)
        if (intent?.action == ACTION_STOP) { lifecycleScope.launch { stopAndSeal("user_stop") }; return START_NOT_STICKY }
        startForegroundNow()
        if (captureJob?.isActive != true) captureJob = lifecycleScope.launch { beginCapture() }
        return START_NOT_STICKY
    }

    @SuppressLint("MissingPermission") // requirePermissions() is the first operation and revocation is caught below.
    private suspend fun beginCapture() {
        try {
            requirePermissions()
            CaptureStatusStore.set(CaptureUiState(phase = "initializing", message = "Creating signed capture session", queued = queue.pendingCount()))
            val preferences = getSharedPreferences(PREFERENCES, MODE_PRIVATE)
            val storedFingerprint = preferences.getString("fingerprint", null)
            val storedAssurance = preferences.getString("assurance", null)
            val identity: dev.proofline.capture.security.DeviceIdentity
            val fingerprint: String
            val assurance: String
            if (!Crypto.hasDeviceIdentity() || storedFingerprint == null || storedAssurance == null) {
                val enrollmentChallenge = api.challenge()
                identity = Crypto.ensureDeviceIdentity(Crypto.decodeBase64Url(enrollmentChallenge.challenge))
                val attested = runCatching { api.attest(AttestationRequest(publicKeySpki = identity.publicKeySpki, certificateChain = identity.certificateChain, challenge = enrollmentChallenge.challenge, claimedAssurance = identity.requestedAssurance)) }
                    .getOrElse {
                        // The fallback is explicit in the public record. A failed or unavailable
                        // verifier never becomes an unearned hardware-attested assurance label.
                        api.attest(AttestationRequest(publicKeySpki = identity.publicKeySpki, certificateChain = null, challenge = enrollmentChallenge.challenge, claimedAssurance = "web_key"))
                    }
                fingerprint = attested.fingerprint; assurance = attested.assuranceLevel
                preferences.edit().putString("fingerprint", fingerprint).putString("assurance", assurance).putString("spki", identity.publicKeySpki).apply()
            } else {
                identity = Crypto.ensureDeviceIdentity(ByteArray(32)); fingerprint = storedFingerprint; assurance = storedAssurance
            }
            val sessionNonce = api.challenge()
            val sessionKey = Crypto.newSessionKey()
            val declarations = CaptureEngine.declarations(this)
            val startedAt = Instant.now().toString()
            val binding = SessionBinding(challenge = sessionNonce.challenge, deviceFingerprint = fingerprint, sessionPublicKeySpki = Crypto.sessionSpki(sessionKey), startedAt = startedAt, streams = declarations)
            val bindingSignature = Crypto.signDevice(CanonicalJson.encode(CanonicalJson.json.encodeToJsonElement(binding)))
            telemetry.start(); delay(750)
            val location = telemetry.location?.let { GeoPoint(it.latitude, it.longitude, it.accuracy) }
            val session = api.createCapture(CreateCaptureRequest(
                sessionNonce = sessionNonce.challenge, deviceFingerprint = fingerprint, assuranceLevel = assurance,
                devicePublicKeySpki = identity.publicKeySpki, sessionPublicKeySpki = Crypto.sessionSpki(sessionKey),
                sessionBindingSignature = bindingSignature, title = "Android field capture", startedAt = startedAt,
                streams = declarations, location = location,
            ))
            currentSession = session; currentSessionKey = sessionKey
            uploader = lifecycleScope.launch { FragmentUploader(queue, api).run() }
            val newEngine = CaptureEngine(this, this, lifecycleScope, sessionKey, session, queue, telemetry, declarations)
            engine = newEngine; startedAtMs = System.currentTimeMillis()
            val cameraCount = newEngine.start()
            CaptureStatusStore.set(CaptureUiState(phase = "recording", message = "Public capture is live", captureId = session.captureId, fingerprint = fingerprint, assurance = assurance, queued = queue.pendingCount(), cameras = cameraCount))
            while (engine === newEngine) {
                val elapsed = System.currentTimeMillis() - startedAtMs
                CaptureStatusStore.update { it.copy(elapsedMs = elapsed, queued = queue.pendingCount()) }
                updateNotification()
                if (elapsed >= 3_600_000) { stopAndSeal("duration_limit"); break }
                delay(1_000)
            }
        } catch (error: Throwable) {
            telemetry.stop()
            CaptureStatusStore.update { it.copy(phase = "error", message = error.message ?: error.javaClass.simpleName, fatal = true) }
            updateNotification()
            stopSelf()
        }
    }

    private suspend fun stopAndSeal(reason: String) {
        val current = engine ?: return
        engine = null
        CaptureStatusStore.update { it.copy(phase = "stopping", message = "Signing the capture ending") }
        val streams = current.stop()
        telemetry.stop()
        val uploadDeadline = System.currentTimeMillis() + 30_000
        while (queue.pendingCount() > 0 && System.currentTimeMillis() < uploadDeadline) delay(100)
        val state = CaptureStatusStore.state.value
        val manifest = EndManifest(captureId = state.captureId!!, endedAt = Instant.now().toString(), durationMs = System.currentTimeMillis() - startedAtMs, closeReason = reason, streams = streams)
        val session = currentSession ?: error("Capture session is missing")
        currentSessionKey ?: error("Session signing key is missing")
        val signature = Crypto.signDevice(CanonicalJson.encode(CanonicalJson.json.encodeToJsonElement(manifest)))
        val endRequest = EndRequest(manifest, signature)
        val sealed = runCatching { api.endCapture(session, endRequest) }.isSuccess
        if (!sealed) {
            queue.enqueueEnd(session, endRequest)
            EvidenceUploadWorker.schedule(this)
        }
        CaptureStatusStore.update { it.copy(phase = if (sealed) "sealed" else "interrupted", message = if (sealed) "Signed ending accepted" else "Ending was not acknowledged; server receipts remain valid") }
        uploader?.cancel(); stopForeground(STOP_FOREGROUND_DETACH); updateNotification(); stopSelf()
    }

    private fun requirePermissions() {
        check(ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) { "Camera permission is required" }
        check(ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) { "Microphone permission is required" }
        check(ContextCompat.checkSelfPermission(this, Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED) { "Precise location permission is required" }
    }

    private fun startForegroundNow() {
        ServiceCompat.startForeground(this, NOTIFICATION_ID, notification(), android.content.pm.ServiceInfo.FOREGROUND_SERVICE_TYPE_CAMERA or android.content.pm.ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE or android.content.pm.ServiceInfo.FOREGROUND_SERVICE_TYPE_LOCATION)
    }
    private fun updateNotification() { getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, notification()) }
    private fun notification(): android.app.Notification {
        val open = PendingIntent.getActivity(this, 1, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        val stop = PendingIntent.getService(this, 2, Intent(this, CaptureForegroundService::class.java).setAction(ACTION_STOP), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        val state = CaptureStatusStore.state.value
        return NotificationCompat.Builder(this, CHANNEL).setSmallIcon(android.R.drawable.presence_video_online).setContentTitle("ProofLine · ${state.phase}")
            .setContentText("${state.acknowledged} acknowledged · ${state.queued} queued").setContentIntent(open).setOngoing(state.phase == "recording")
            .addAction(android.R.drawable.ic_media_pause, "Stop and seal", stop).build()
    }
    private fun createChannel() { getSystemService(NotificationManager::class.java).createNotificationChannel(NotificationChannel(CHANNEL, "Public evidence capture", NotificationManager.IMPORTANCE_HIGH)) }

    companion object {
        const val ACTION_START = "dev.proofline.capture.START"
        const val ACTION_STOP = "dev.proofline.capture.STOP"
        private const val CHANNEL = "proofline-capture-v2"; private const val NOTIFICATION_ID = 2202; private const val PREFERENCES = "proofline-device-v2"
    }
}
