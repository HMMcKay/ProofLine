package dev.proofline.capture.storage

import android.content.ContentValues
import android.content.Context
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteOpenHelper
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import dev.proofline.capture.protocol.CanonicalJson
import dev.proofline.capture.protocol.CreateCaptureResponse
import dev.proofline.capture.protocol.EndRequest
import dev.proofline.capture.protocol.FragmentEnvelope
import dev.proofline.capture.protocol.TelemetryBatch
import kotlinx.serialization.encodeToString
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

data class QueuedFragment(
    val id: Long, val session: CreateCaptureResponse, val envelope: FragmentEnvelope,
    val signature: String, val encryptedFile: File,
)
data class QueuedTelemetry(val id: Long, val session: CreateCaptureResponse, val batch: TelemetryBatch, val encryptedFile: File)
data class QueuedEnd(val id: Long, val session: CreateCaptureResponse, val request: EndRequest, val encryptedFile: File)

/**
 * Unacknowledged media is encrypted with an Android Keystore AES key before it
 * enters the queue. When capacity is exhausted capture stops loudly; this class
 * never evicts evidence that the server has not acknowledged.
 */
class EncryptedFragmentQueue(context: Context) : SQLiteOpenHelper(context, "proofline-queue.db", null, 3) {
    private val appContext = context.applicationContext
    private val directory = File(context.noBackupFilesDir, "evidence-queue").apply { mkdirs() }
    private val key: SecretKey by lazy { queueKey() }

    override fun onCreate(db: SQLiteDatabase) {
        db.execSQL("CREATE TABLE fragments(id INTEGER PRIMARY KEY AUTOINCREMENT, capture_id TEXT NOT NULL, stream_id TEXT NOT NULL, sequence INTEGER NOT NULL, session_json TEXT NOT NULL, envelope_json TEXT NOT NULL, signature TEXT NOT NULL, path TEXT NOT NULL, byte_length INTEGER NOT NULL, queued_at INTEGER NOT NULL, UNIQUE(capture_id,stream_id,sequence))")
        createTelemetryTable(db)
        createEndTable(db)
    }
    override fun onUpgrade(db: SQLiteDatabase, oldVersion: Int, newVersion: Int) {
        if (oldVersion < 2) createTelemetryTable(db)
        if (oldVersion < 3) createEndTable(db)
    }

    private fun createTelemetryTable(db: SQLiteDatabase) {
        db.execSQL("CREATE TABLE IF NOT EXISTS telemetry(id INTEGER PRIMARY KEY AUTOINCREMENT, capture_id TEXT NOT NULL, sequence INTEGER NOT NULL, session_json TEXT NOT NULL, path TEXT NOT NULL, byte_length INTEGER NOT NULL, queued_at INTEGER NOT NULL, UNIQUE(capture_id,sequence))")
    }
    private fun createEndTable(db: SQLiteDatabase) {
        db.execSQL("CREATE TABLE IF NOT EXISTS endings(id INTEGER PRIMARY KEY AUTOINCREMENT, capture_id TEXT NOT NULL UNIQUE, session_json TEXT NOT NULL, path TEXT NOT NULL, byte_length INTEGER NOT NULL, queued_at INTEGER NOT NULL)")
    }

    @Synchronized fun enqueue(session: CreateCaptureResponse, envelope: FragmentEnvelope, signature: String, bytes: ByteArray) {
        val projected = pendingBytes() + bytes.size
        if (projected > MAX_QUEUE_BYTES) throw QueueCapacityException("Encrypted queue reached its 2 GiB safety limit")
        val file = File(directory, "${session.captureId}-${envelope.streamId}-${envelope.sequence}.plq")
        file.writeBytes(encrypt(bytes))
        val values = ContentValues().apply {
            put("capture_id", session.captureId); put("stream_id", envelope.streamId); put("sequence", envelope.sequence)
            put("session_json", CanonicalJson.json.encodeToString(session)); put("envelope_json", CanonicalJson.json.encodeToString(envelope))
            put("signature", signature); put("path", file.absolutePath); put("byte_length", bytes.size); put("queued_at", System.currentTimeMillis())
        }
        try { writableDatabase.insertOrThrow("fragments", null, values) }
        catch (error: Throwable) { file.delete(); throw error }
        EvidenceUploadWorker.schedule(appContext)
    }

    @Synchronized fun oldest(limit: Int = 32): List<QueuedFragment> {
        val cursor = readableDatabase.rawQuery("SELECT id,session_json,envelope_json,signature,path FROM fragments ORDER BY queued_at,sequence LIMIT ?", arrayOf(limit.toString()))
        return cursor.use {
            buildList {
                while (it.moveToNext()) add(QueuedFragment(it.getLong(0), CanonicalJson.json.decodeFromString(it.getString(1)), CanonicalJson.json.decodeFromString(it.getString(2)), it.getString(3), File(it.getString(4))))
            }
        }
    }

    @Synchronized fun enqueueTelemetry(session: CreateCaptureResponse, batch: TelemetryBatch) {
        val plain = CanonicalJson.json.encodeToString(batch).toByteArray()
        if (pendingBytes() + plain.size > MAX_QUEUE_BYTES) throw QueueCapacityException("Encrypted queue reached its 2 GiB safety limit")
        val file = File(directory, "${session.captureId}-telemetry-${batch.sequence}.plq")
        file.writeBytes(encrypt(plain))
        val values = ContentValues().apply {
            put("capture_id", session.captureId); put("sequence", batch.sequence)
            put("session_json", CanonicalJson.json.encodeToString(session)); put("path", file.absolutePath)
            put("byte_length", plain.size); put("queued_at", System.currentTimeMillis())
        }
        try { writableDatabase.insertOrThrow("telemetry", null, values) }
        catch (error: Throwable) { file.delete(); throw error }
        EvidenceUploadWorker.schedule(appContext)
    }

    @Synchronized fun oldestTelemetry(): QueuedTelemetry? {
        val cursor = readableDatabase.rawQuery("SELECT id,session_json,path FROM telemetry ORDER BY queued_at,sequence LIMIT 1", null)
        return cursor.use {
            if (!it.moveToFirst()) null else {
                val file = File(it.getString(2))
                val batch = CanonicalJson.json.decodeFromString<TelemetryBatch>(decrypt(file.readBytes()).decodeToString())
                QueuedTelemetry(it.getLong(0), CanonicalJson.json.decodeFromString(it.getString(1)), batch, file)
            }
        }
    }

    @Synchronized fun enqueueEnd(session: CreateCaptureResponse, request: EndRequest) {
        val plain = CanonicalJson.json.encodeToString(request).toByteArray()
        val file = File(directory, "${session.captureId}-ending.plq")
        file.writeBytes(encrypt(plain))
        val values = ContentValues().apply {
            put("capture_id", session.captureId); put("session_json", CanonicalJson.json.encodeToString(session))
            put("path", file.absolutePath); put("byte_length", plain.size); put("queued_at", System.currentTimeMillis())
        }
        try { writableDatabase.insertWithOnConflict("endings", null, values, SQLiteDatabase.CONFLICT_REPLACE) }
        catch (error: Throwable) { file.delete(); throw error }
        EvidenceUploadWorker.schedule(appContext)
    }

    @Synchronized fun oldestEnd(): QueuedEnd? {
        val cursor = readableDatabase.rawQuery("SELECT id,session_json,path FROM endings ORDER BY queued_at LIMIT 1", null)
        return cursor.use {
            if (!it.moveToFirst()) null else {
                val file = File(it.getString(2))
                val request = CanonicalJson.json.decodeFromString<EndRequest>(decrypt(file.readBytes()).decodeToString())
                QueuedEnd(it.getLong(0), CanonicalJson.json.decodeFromString(it.getString(1)), request, file)
            }
        }
    }

    fun decrypt(fragment: QueuedFragment): ByteArray = decrypt(fragment.encryptedFile.readBytes())

    @Synchronized fun acknowledge(fragment: QueuedFragment) {
        writableDatabase.delete("fragments", "id=?", arrayOf(fragment.id.toString()))
        fragment.encryptedFile.delete()
    }

    @Synchronized fun acknowledge(telemetry: QueuedTelemetry) {
        writableDatabase.delete("telemetry", "id=?", arrayOf(telemetry.id.toString()))
        telemetry.encryptedFile.delete()
    }
    @Synchronized fun acknowledge(end: QueuedEnd) {
        writableDatabase.delete("endings", "id=?", arrayOf(end.id.toString()))
        end.encryptedFile.delete()
    }

    @Synchronized fun pendingCount(): Int = readableDatabase.rawQuery("SELECT (SELECT count(*) FROM fragments)+(SELECT count(*) FROM telemetry)+(SELECT count(*) FROM endings)", null).use { it.moveToFirst(); it.getInt(0) }
    @Synchronized fun pendingBytes(): Long = readableDatabase.rawQuery("SELECT coalesce((SELECT sum(byte_length) FROM fragments),0)+coalesce((SELECT sum(byte_length) FROM telemetry),0)+coalesce((SELECT sum(byte_length) FROM endings),0)", null).use { it.moveToFirst(); it.getLong(0) }

    private fun encrypt(plain: ByteArray): ByteArray {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.ENCRYPT_MODE, key) }
        return cipher.iv + cipher.doFinal(plain)
    }
    private fun decrypt(value: ByteArray): ByteArray {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, value.copyOfRange(0, 12))) }
        return cipher.doFinal(value.copyOfRange(12, value.size))
    }
    private fun queueKey(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
        val spec = KeyGenParameterSpec.Builder(KEY_ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).build()
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply { init(spec) }.generateKey()
    }
    companion object { private const val KEY_ALIAS = "proofline-local-queue-v2"; private const val MAX_QUEUE_BYTES = 2L * 1024 * 1024 * 1024 }
}

class QueueCapacityException(message: String) : IllegalStateException(message)
