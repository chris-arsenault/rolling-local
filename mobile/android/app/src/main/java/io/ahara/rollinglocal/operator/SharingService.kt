package io.ahara.rollinglocal.operator

import android.Manifest.permission.ACCESS_COARSE_LOCATION
import android.Manifest.permission.ACCESS_FINE_LOCATION
import android.app.Application
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.location.Location
import android.location.LocationListener
import android.location.LocationManager
import android.os.Bundle
import android.os.IBinder
import android.os.Looper
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.time.Duration
import java.time.Instant
import java.util.UUID

class RollingApp : Application() {
    val model by lazy { OperatorModel(this) }
}

data class OperatorScreen(
    val state: OperatorState = OperatorState(),
    val signedIn: Boolean = false,
    val busy: Boolean = false,
    val starting: Boolean = false,
    val message: String? = null,
)

class OperatorModel(private val context: Context) {
    val client = OperatorClient(context)
    val scope = CoroutineScope(Dispatchers.Main.immediate + SupervisorJob())
    private val screen = MutableStateFlow(OperatorScreen(signedIn = client.signedIn))
    val ui = screen.asStateFlow()

    suspend fun refresh() {
        if (!client.signedIn) return
        operation { apply(client.state()) }
    }

    suspend fun signedIn(data: Intent) = operation {
        client.signInResult(data)
        apply(client.state())
    }

    suspend fun signOut() {
        stop()
        client.signOut()
        screen.value = OperatorScreen()
        widget()
    }

    suspend fun save(settings: Settings): Boolean = operation {
        val previous = screen.value.state.session
        val state = client.settings(settings)
        apply(state)
        if (previous != null && state.session == null) context.stopService(Intent(context, SharingService::class.java))
    }

    suspend fun publish(photo: ByteArray, captured: Instant): Boolean = operation {
        client.publish(photo, captured)
        apply(client.state())
        message("Menu published.")
    }

    fun start() {
        if (screen.value.starting || screen.value.state.session != null) return
        screen.update { it.copy(starting = true, message = null) }
        try {
            ContextCompat.startForegroundService(context, Intent(context, SharingService::class.java))
        } catch (_: SecurityException) {
            finished("Allow location access to start sharing.")
        } catch (_: IllegalStateException) {
            finished("Open Rolling Local and try starting again.")
        }
    }

    suspend fun stop() {
        val id = screen.value.state.session?.id
        context.stopService(Intent(context, SharingService::class.java))
        finished(null)
        if (id != null) retire(id)
    }

    fun sharing(session: Sharing) {
        screen.update { it.copy(state = it.state.copy(session = session), starting = false, message = null) }
        widget()
    }

    fun finished(reason: String?) {
        screen.update { it.copy(state = it.state.copy(session = null), starting = false, message = reason) }
        widget()
    }

    suspend fun retire(id: String) {
        try {
            client.stop(id)
        } catch (_: Exception) {
            message("Phone sharing stopped. The last public position will expire if the server cannot be reached.")
        }
    }

    fun message(text: String) {
        screen.update { it.copy(message = text) }
    }

    private suspend fun operation(work: suspend () -> Unit): Boolean {
        if (screen.value.busy) return false
        screen.update { it.copy(busy = true, message = null) }
        return try {
            work()
            true
        } catch (error: Exception) {
            screen.update { it.copy(signedIn = client.signedIn && (error as? ServerError)?.code != 401) }
            message(error.message ?: "The request could not be completed.")
            false
        } finally {
            screen.update { it.copy(busy = false) }
        }
    }

    private fun apply(state: OperatorState) {
        screen.update { it.copy(signedIn = client.signedIn, state = state) }
        widget()
    }

    private fun widget() {
        val state = screen.value.state
        OperatorWidget.save(context, state.profile?.settings?.name, state.session?.expiresAt)
    }
}

class SharingService : Service(), LocationListener {
    private val model get() = (application as RollingApp).model
    private val locations get() = getSystemService(LocationManager::class.java)
    private var id: UUID? = null
    private var session: Sharing? = null
    private var sending = false
    private var deadline: Job? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            finish("Sharing stopped.")
            return START_NOT_STICKY
        }
        if (id != null) return START_NOT_STICKY
        startForeground(NOTICE_ID, notification("Finding a current location"))
        id = UUID.randomUUID()
        val subscribed = subscribe()
        if (!subscribed) finish("Allow location access and turn on location services.")
        else schedule(30_000, "Could not get a current location. Try again outdoors.")
        return START_NOT_STICKY
    }

    private fun subscribe(): Boolean {
        val fine = checkSelfPermission(ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED
        val coarse = checkSelfPermission(ACCESS_COARSE_LOCATION) == PackageManager.PERMISSION_GRANTED
        if (!fine && !coarse) return false
        val providers = if (fine) listOf(LocationManager.GPS_PROVIDER, LocationManager.NETWORK_PROVIDER)
            else listOf(LocationManager.NETWORK_PROVIDER)
        return try {
            val enabled = providers.filter { locations.isProviderEnabled(it) }
            enabled.forEach { locations.requestLocationUpdates(it, 10_000L, 0f, this, Looper.getMainLooper()) }
            enabled.isNotEmpty()
        } catch (_: SecurityException) {
            false
        }
    }

    override fun onProviderDisabled(provider: String) {
        val anyEnabled = listOf(LocationManager.GPS_PROVIDER, LocationManager.NETWORK_PROVIDER)
            .any { locations.isProviderEnabled(it) }
        if (!anyEnabled) finish("Sharing stopped when location became unavailable.")
    }

    override fun onProviderEnabled(provider: String) = Unit

    @Deprecated("Legacy callback for Android 9 and earlier")
    override fun onStatusChanged(provider: String?, status: Int, extras: Bundle?) = Unit

    override fun onLocationChanged(location: Location) {
        val attempt = id ?: return
        val settings = model.ui.value.state.profile?.settings ?: return
        val now = Instant.now()
        val current = session
        if (current?.expired(now) == true) {
            finish("Sharing expired.")
            return
        }
        if (!location.hasAccuracy() || Duration.between(Instant.ofEpochMilli(location.time), now).abs() > Duration.ofMinutes(2)) return
        val area = settings.event?.takeIf { Instant.parse(it.expiresAt) > now }?.area ?: settings.area
        val distance = FloatArray(1)
        Location.distanceBetween(location.latitude, location.longitude, area.center.latitude, area.center.longitude, distance)
        if (distance[0] + location.accuracy > area.radiusMeters) {
            finish("Sharing stopped outside your operating area.")
            return
        }
        if (sending || (current != null && now < Instant.parse(current.publishedAt).plusSeconds(600))) return
        val fix = Fix(Position(location.latitude, location.longitude), location.accuracy.toDouble(),
            Instant.ofEpochMilli(location.time).toString())
        send(attempt, fix, current)
    }

    private fun send(attempt: UUID, fix: Fix, current: Sharing?) {
        sending = true
        model.scope.launch {
            try {
                val next = if (current == null) model.client.start(attempt, fix) else model.client.update(attempt, fix)
                if (id != attempt) {
                    model.retire(attempt.toString())
                    return@launch
                }
                session = next
                model.sharing(next)
                getSystemService(NotificationManager::class.java).notify(NOTICE_ID, notification("Location sharing is on"))
                schedule(Duration.between(Instant.now(), Instant.parse(next.expiresAt)).toMillis(), "Sharing expired.")
            } catch (error: Exception) {
                if (attempt != id) return@launch
                if (current == null || (error as? ServerError)?.code in listOf(400, 401, 403, 409)) {
                    finish(error.message ?: "Sharing could not start.")
                } else {
                    model.message("Update failed. The last position expires at the time shown.")
                }
            } finally {
                sending = false
            }
        }
    }

    private fun schedule(milliseconds: Long, reason: String) {
        deadline?.cancel()
        deadline = model.scope.launch {
            delay(milliseconds.coerceAtLeast(0))
            finish(reason)
        }
    }

    private fun finish(reason: String) {
        val stop = id
        id = null
        session = null
        deadline?.cancel()
        locations.removeUpdates(this)
        model.finished(reason)
        if (stop != null) model.scope.launch { model.retire(stop.toString()) }
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onDestroy() {
        val stop = id
        id = null
        deadline?.cancel()
        locations.removeUpdates(this)
        if (stop != null) model.scope.launch { model.retire(stop.toString()) }
        super.onDestroy()
    }

    private fun notification(text: String): Notification {
        val service = getSystemService(NotificationManager::class.java)
        service.createNotificationChannel(NotificationChannel(CHANNEL, "Location sharing", NotificationManager.IMPORTANCE_LOW))
        val open = PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
        val stop = PendingIntent.getService(this, 1, Intent(this, SharingService::class.java).setAction(ACTION_STOP), PendingIntent.FLAG_IMMUTABLE)
        return Notification.Builder(this, CHANNEL).setSmallIcon(android.R.drawable.ic_menu_mylocation)
            .setContentTitle("Rolling Local").setContentText(text).setContentIntent(open)
            .addAction(Notification.Action.Builder(null, "Stop sharing", stop).build())
            .setOngoing(true).build()
    }

    companion object {
        private const val NOTICE_ID = 501
        private const val CHANNEL = "rolling-local-sharing"
        private const val ACTION_STOP = "io.ahara.rollinglocal.stop"
    }
}
