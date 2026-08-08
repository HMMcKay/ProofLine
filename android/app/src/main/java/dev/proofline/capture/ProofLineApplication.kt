package dev.proofline.capture

import android.app.Application
import dev.proofline.capture.capture.CaptureStatusStore

class ProofLineApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        CaptureStatusStore.initialize(this)
    }
}
