package dev.proofline.capture.capture

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

data class CaptureUiState(
    val phase: String = "idle", val message: String = "Ready",
    val captureId: String? = null, val fingerprint: String? = null, val assurance: String? = null,
    val acknowledged: Long = 0, val queued: Int = 0, val cameras: Int = 0, val elapsedMs: Long = 0,
    val fatal: Boolean = false,
)

object CaptureStatusStore {
    private val mutable = MutableStateFlow(CaptureUiState())
    val state: StateFlow<CaptureUiState> = mutable.asStateFlow()
    private var appContext: Context? = null
    fun initialize(context: Context) { appContext = context.applicationContext }
    fun update(transform: (CaptureUiState) -> CaptureUiState) { mutable.value = transform(mutable.value) }
    fun set(value: CaptureUiState) { mutable.value = value }
}
