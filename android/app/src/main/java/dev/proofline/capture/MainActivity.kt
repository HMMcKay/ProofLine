package dev.proofline.capture

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import dev.proofline.capture.capture.CaptureForegroundService
import dev.proofline.capture.capture.CaptureStatusStore
import dev.proofline.capture.capture.CaptureUiState

class MainActivity : ComponentActivity() {
    private val preferences by lazy { getSharedPreferences("proofline-launch-v2", MODE_PRIVATE) }
    private val permissionLauncher = registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { result ->
        if (REQUIRED.all { result[it] == true || ContextCompat.checkSelfPermission(this, it) == PackageManager.PERMISSION_GRANTED }) startCapture()
        else CaptureStatusStore.update { it.copy(phase = "error", message = "Camera, microphone, and precise location permissions are required", fatal = true) }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent { ProofLineTheme { App(preferences.getBoolean("public_warning_accepted", false)) } }
        if (preferences.getBoolean("public_warning_accepted", false)) requestOrStart()
    }

    private fun acceptWarning() {
        preferences.edit().putBoolean("public_warning_accepted", true).apply()
        recreate()
    }
    private fun requestOrStart() {
        if (REQUIRED.all { ContextCompat.checkSelfPermission(this, it) == PackageManager.PERMISSION_GRANTED }) startCapture()
        else permissionLauncher.launch((REQUIRED + if (Build.VERSION.SDK_INT >= 33) listOf(Manifest.permission.POST_NOTIFICATIONS) else emptyList()).toTypedArray())
    }
    private fun startCapture() {
        ContextCompat.startForegroundService(this, Intent(this, CaptureForegroundService::class.java).setAction(CaptureForegroundService.ACTION_START))
    }
    private fun stopCapture() { startService(Intent(this, CaptureForegroundService::class.java).setAction(CaptureForegroundService.ACTION_STOP)) }
    private fun openMyVideos(fingerprint: String?) {
        val target = if (fingerprint == null) "${BuildConfig.PROOFLINE_CONTROL_URL}/my" else "${BuildConfig.PROOFLINE_CONTROL_URL}/devices/$fingerprint"
        startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(target)))
    }

    @Composable private fun App(consented: Boolean) {
        val state by CaptureStatusStore.state.collectAsState()
        if (!consented) PublicWarning(::acceptWarning) else CaptureScreen(state, ::stopCapture, ::openMyVideos)
    }

    companion object {
        private val REQUIRED = listOf(Manifest.permission.CAMERA, Manifest.permission.RECORD_AUDIO, Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION)
    }
}

@Composable internal fun PublicWarning(onAccept: () -> Unit) {
    Column(Modifier.fillMaxSize().background(Color(0xFF0A0B0E)).verticalScroll(rememberScrollState()).padding(24.dp), verticalArrangement = Arrangement.Center) {
        Text("PROOFLINE", color = Color(0xFFED594B), fontWeight = FontWeight.Bold, letterSpacing = 3.sp)
        Spacer(Modifier.height(20.dp))
        Text("Opening this app records in public.", color = Color.White, fontSize = 36.sp, lineHeight = 40.sp, fontWeight = FontWeight.Black)
        Spacer(Modifier.height(20.dp))
        Surface(color = Color(0xFF351513), shape = MaterialTheme.shapes.medium) {
            Column(Modifier.padding(18.dp)) {
                Text("There is no private mode.", color = Color(0xFFFF9D92), fontWeight = FontWeight.Bold)
                Text("After this one-time setup, every explicit launch begins recording and uploading immediately. Closing the app, losing the phone, or deleting local files cannot retract fragments already acknowledged by the server.", color = Color.White, modifier = Modifier.padding(top = 8.dp))
            }
        }
        Spacer(Modifier.height(18.dp))
        Text("• Video, microphone audio, exact location, motion, orientation, pressure, device state, and network evidence are recorded.\n\n• Exact coordinates are hidden while live and released 30 minutes after capture ends.\n\n• Every capture is publicly viewable under a pseudonymous device fingerprint.\n\n• ProofLine provides provenance evidence, not a guarantee that a scene is truthful or legally admissible.", color = Color(0xFFD0D3D8), lineHeight = 22.sp)
        Spacer(Modifier.height(24.dp))
        Button(onClick = onAccept, colors = ButtonDefaults.buttonColors(containerColor = Color(0xFFED594B)), modifier = Modifier.fillMaxWidth()) { Text("I understand — configure ProofLine", color = Color.Black, fontWeight = FontWeight.Bold) }
    }
}

@Composable private fun CaptureScreen(state: CaptureUiState, onStop: () -> Unit, onMyVideos: (String?) -> Unit) {
    Column(Modifier.fillMaxSize().background(Color(0xFF0A0B0E)).verticalScroll(rememberScrollState()).padding(24.dp)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
            Text("PROOFLINE", color = Color(0xFFED594B), fontWeight = FontWeight.Bold, letterSpacing = 3.sp)
            Text(state.phase.uppercase(), color = if (state.phase == "recording") Color(0xFFFF6B61) else Color(0xFFB6BBC5), fontWeight = FontWeight.Bold)
        }
        Spacer(Modifier.height(42.dp))
        Text(if (state.phase == "recording") "Recording publicly" else state.phase.replaceFirstChar { it.uppercase() }, color = Color.White, fontSize = 34.sp, fontWeight = FontWeight.Black)
        Text(state.message, color = if (state.fatal) Color(0xFFFF8E84) else Color(0xFFB6BBC5), modifier = Modifier.padding(top = 8.dp))
        Spacer(Modifier.height(28.dp))
        Metric("Capture", state.captureId ?: "initializing")
        Metric("Assurance", state.assurance ?: "pending verification")
        Metric("Cameras", state.cameras.toString())
        Metric("Acknowledged", state.acknowledged.toString())
        Metric("Encrypted queue", state.queued.toString())
        Metric("Elapsed", "%02d:%02d".format(state.elapsedMs / 60_000, state.elapsedMs / 1000 % 60))
        Spacer(Modifier.height(24.dp))
        if (state.phase == "recording") Button(onClick = onStop, modifier = Modifier.fillMaxWidth(), colors = ButtonDefaults.buttonColors(containerColor = Color(0xFFED594B))) { Text("Stop and sign ending", color = Color.Black, fontWeight = FontWeight.Bold) }
        Spacer(Modifier.height(10.dp))
        OutlinedButton(onClick = { onMyVideos(state.fingerprint) }, modifier = Modifier.fillMaxWidth()) { Text("Open public device history") }
    }
}

@Composable private fun Metric(label: String, value: String) {
    Row(Modifier.fillMaxWidth().padding(vertical = 10.dp), horizontalArrangement = Arrangement.SpaceBetween) {
        Text(label, color = Color(0xFF8D939E)); Text(value.take(28), color = Color.White, fontWeight = FontWeight.SemiBold)
    }
}

@Composable private fun ProofLineTheme(content: @Composable () -> Unit) { MaterialTheme(colorScheme = darkColorScheme(primary = Color(0xFFED594B)), content = content) }
