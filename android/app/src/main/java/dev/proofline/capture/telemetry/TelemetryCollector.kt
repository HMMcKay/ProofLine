package dev.proofline.capture.telemetry

import android.annotation.SuppressLint
import android.content.Context
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.location.Location
import android.os.BatteryManager
import android.os.SystemClock
import com.google.android.gms.location.LocationServices
import com.google.android.gms.location.Priority
import dev.proofline.capture.protocol.CanonicalJson
import dev.proofline.capture.protocol.GENESIS_DIGEST
import dev.proofline.capture.security.Crypto
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import java.util.concurrent.ConcurrentLinkedQueue

data class DrainedTelemetry(val samples: JsonArray, val root: String)

class TelemetryCollector(private val context: Context) : SensorEventListener {
    private val sensorManager = context.getSystemService(SensorManager::class.java)
    private val samples = ConcurrentLinkedQueue<JsonObject>()
    @Volatile private var lastRoot = GENESIS_DIGEST
    @Volatile var location: Location? = null; private set

    @SuppressLint("MissingPermission")
    fun start() {
        listOf(Sensor.TYPE_ACCELEROMETER, Sensor.TYPE_GYROSCOPE, Sensor.TYPE_PRESSURE, Sensor.TYPE_ROTATION_VECTOR).forEach { type ->
            sensorManager.getDefaultSensor(type)?.let { sensorManager.registerListener(this, it, SensorManager.SENSOR_DELAY_GAME) }
        }
        LocationServices.getFusedLocationProviderClient(context).getCurrentLocation(Priority.PRIORITY_HIGH_ACCURACY, null)
            .addOnSuccessListener { location = it }
        addLifecycle("telemetry_started")
    }

    fun stop() { addLifecycle("telemetry_stopped"); sensorManager.unregisterListener(this) }

    fun addLifecycle(event: String) {
        samples.add(JsonObject(mapOf(
            "kind" to JsonPrimitive("lifecycle"), "event" to JsonPrimitive(event),
            "elapsedRealtimeNanos" to JsonPrimitive(SystemClock.elapsedRealtimeNanos()), "wallTimeMs" to JsonPrimitive(System.currentTimeMillis()),
        )))
    }

    @Synchronized fun drainBatch(): DrainedTelemetry {
        val drained = buildList { while (true) add(samples.poll() ?: break) }
        if (drained.isEmpty()) return DrainedTelemetry(JsonArray(emptyList()), lastRoot)
        val payload = JsonArray(drained)
        lastRoot = Crypto.sha256Hex(CanonicalJson.encode(payload).toByteArray())
        return DrainedTelemetry(payload, lastRoot)
    }

    override fun onSensorChanged(event: SensorEvent) {
        samples.add(JsonObject(mapOf(
            "kind" to JsonPrimitive("sensor"), "sensor" to JsonPrimitive(event.sensor.stringType),
            "sensorTimestampNanos" to JsonPrimitive(event.timestamp), "elapsedRealtimeNanos" to JsonPrimitive(SystemClock.elapsedRealtimeNanos()),
            "wallTimeMs" to JsonPrimitive(System.currentTimeMillis()), "accuracy" to JsonPrimitive(event.accuracy),
            "values" to JsonArray(event.values.map(::JsonPrimitive)),
            "thermalStatus" to JsonPrimitive(context.getSystemService(android.os.PowerManager::class.java).currentThermalStatus),
            "batteryPercent" to JsonPrimitive(context.getSystemService(BatteryManager::class.java).getIntProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY)),
        )))
        while (samples.size > 20_000) samples.poll()
    }
    override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) = Unit
}
