package com.laffy.unifiedstream.session

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import android.util.Log
import androidx.core.app.NotificationCompat
import com.laffy.unifiedstream.MainActivity
import com.laffy.unifiedstream.R

private const val TAG = "SessionService"
private const val CHANNEL_ID = "unifiedstream_session"
private const val NOTIFICATION_ID = 1

/**
 * Keeps a session alive while the app is not in the foreground.
 *
 * The phone is a webcam precisely when the user has switched to another app, so the session
 * cannot live in the Activity. Android would otherwise kill the sockets the moment the screen
 * changes. The cost is a permanent notification, which is the accepted price.
 */
class SessionService : android.app.Service() {

    private var wakeLock: PowerManager.WakeLock? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        createChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val peer = intent?.getStringExtra(EXTRA_PEER_NAME) ?: "PC"
        startForegroundCompat(peer)
        acquireWakeLock()
        // Restarting with the last intent would resurrect a session the user already ended.
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        releaseWakeLock()
        super.onDestroy()
    }

    private fun startForegroundCompat(peerName: String) {
        val notification = buildNotification(peerName)
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                startForeground(
                    NOTIFICATION_ID,
                    notification,
                    ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE,
                )
            } else {
                startForeground(NOTIFICATION_ID, notification)
            }
        } catch (e: SecurityException) {
            // Missing POST_NOTIFICATIONS or a foreground-service restriction; the session can
            // still run in the foreground, so this must not take the app down.
            Log.w(TAG, "could not enter the foreground", e)
            stopSelf()
        }
    }

    private fun buildNotification(peerName: String): Notification {
        val open = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE,
        )

        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("Connected to $peerName")
            .setContentText("UnifiedStream is keeping the session alive")
            .setSmallIcon(R.drawable.ic_launcher_foreground)
            .setOngoing(true)
            .setSilent(true)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .setContentIntent(open)
            .build()
    }

    private fun createChannel() {
        val channel = NotificationChannel(
            CHANNEL_ID,
            "Streaming session",
            NotificationManager.IMPORTANCE_LOW,
        ).apply {
            description = "Shown while a streaming session is connected"
            setShowBadge(false)
        }
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    private fun acquireWakeLock() {
        if (wakeLock != null) return
        val power = getSystemService(PowerManager::class.java)
        wakeLock = power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "unifiedstream:session")
            .apply {
                setReferenceCounted(false)
                // A partial lock keeps the CPU alive for the sockets without touching the
                // screen; the notification is what makes this visible to the user.
                acquire(WAKE_LOCK_TIMEOUT_MS)
            }
    }

    private fun releaseWakeLock() {
        wakeLock?.let { lock ->
            runCatching { if (lock.isHeld) lock.release() }
                .onFailure { Log.w(TAG, "could not release wake lock", it) }
        }
        wakeLock = null
    }

    companion object {
        private const val EXTRA_PEER_NAME = "peer_name"

        /** A backstop so a leaked lock cannot drain the battery indefinitely. */
        private const val WAKE_LOCK_TIMEOUT_MS = 4L * 60 * 60 * 1000

        /** Start keeping the session alive in the background. */
        fun start(context: Context, peerName: String) {
            val intent = Intent(context, SessionService::class.java)
                .putExtra(EXTRA_PEER_NAME, peerName)
            context.startForegroundService(intent)
        }

        /** Stop the foreground service and drop its notification. */
        fun stop(context: Context) {
            context.stopService(Intent(context, SessionService::class.java))
        }
    }
}
