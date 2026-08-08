package dev.proofline.capture.storage

import android.content.Context
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import dev.proofline.capture.network.ProofLineApi
import kotlinx.coroutines.withTimeoutOrNull
import java.util.concurrent.TimeUnit

/** Resumes evidence delivery without restarting camera or microphone access. */
class EvidenceUploadWorker(context: Context, parameters: WorkerParameters) : CoroutineWorker(context, parameters) {
    override suspend fun doWork(): Result {
        val queue = EncryptedFragmentQueue(applicationContext)
        val drained = withTimeoutOrNull(TimeUnit.MINUTES.toMillis(8)) {
            val uploader = FragmentUploader(queue, ProofLineApi())
            while (queue.pendingCount() > 0) uploader.uploadAvailableOnce()
            true
        } ?: false
        return if (drained && queue.pendingCount() == 0) Result.success() else Result.retry()
    }

    companion object {
        fun schedule(context: Context) {
            val request = OneTimeWorkRequestBuilder<EvidenceUploadWorker>()
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS)
                .build()
            WorkManager.getInstance(context).enqueueUniqueWork("proofline-evidence-upload", ExistingWorkPolicy.KEEP, request)
        }
    }
}
