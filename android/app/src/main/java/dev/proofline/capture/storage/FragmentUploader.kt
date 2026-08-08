package dev.proofline.capture.storage

import dev.proofline.capture.capture.CaptureStatusStore
import dev.proofline.capture.network.ProofLineApi
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlin.coroutines.coroutineContext

class FragmentUploader(private val queue: EncryptedFragmentQueue, private val api: ProofLineApi) {
    suspend fun run() {
        while (coroutineContext.isActive) if (!uploadAvailableOnce()) delay(250)
    }

    /** Attempts one item of each type so a continuous media stream cannot starve
     * telemetry or a signed ending. Returns false only when the queue was empty. */
    suspend fun uploadAvailableOnce(): Boolean {
        val fragment = queue.oldest(1).firstOrNull()
        val telemetry = queue.oldestTelemetry()
        val ending = queue.oldestEnd()
        if (fragment == null && telemetry == null && ending == null) return false
        var failed = false
        try {
            if (fragment != null) {
                api.uploadFragment(fragment.session, fragment.envelope, fragment.signature, queue.decrypt(fragment))
                queue.acknowledge(fragment)
                CaptureStatusStore.update { it.copy(acknowledged = it.acknowledged + 1, queued = queue.pendingCount(), message = "Server acknowledged fragment ${fragment.envelope.sequence}") }
            }
        } catch (_: Throwable) { failed = true }
        try {
            if (telemetry != null) {
                api.uploadTelemetry(telemetry.session, telemetry.batch)
                queue.acknowledge(telemetry)
            }
        } catch (_: Throwable) { failed = true }
        try {
            if (ending != null) {
                api.endCapture(ending.session, ending.request)
                queue.acknowledge(ending)
            }
        } catch (_: Throwable) { failed = true }
        if (failed) {
            CaptureStatusStore.update { it.copy(queued = queue.pendingCount(), message = "Network unavailable; encrypted evidence remains queued") }
            delay(2_000)
        }
        return true
    }
}
