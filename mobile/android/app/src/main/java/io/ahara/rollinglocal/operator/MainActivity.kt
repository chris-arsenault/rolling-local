package io.ahara.rollinglocal.operator

import android.Manifest
import android.app.DatePickerDialog
import android.app.TimePickerDialog
import android.content.ActivityNotFoundException
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Matrix
import android.media.ExifInterface
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.ByteArrayOutputStream
import java.io.File
import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.UUID

class LaunchActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        startActivity(Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP))
        finish()
    }
}

private data class Draft(val jpeg: ByteArray, val captured: Instant)

class MainActivity : ComponentActivity() {
    private val model get() = (application as RollingApp).model
    private var editSettings by mutableStateOf(false)
    private var draft by mutableStateOf<Draft?>(null)
    private var cameraFile: File? = null

    private val login = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        val data = result.data ?: return@registerForActivityResult
        model.scope.launch { model.signedIn(data) }
    }
    private val permission = registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        if (locationAllowed()) model.start() else model.message("Allow location access to start sharing.")
    }
    private val camera = registerForActivityResult(ActivityResultContracts.TakePicture()) { captured ->
        val file = cameraFile ?: return@registerForActivityResult
        cameraFile = null
        model.scope.launch {
            draft = withContext(Dispatchers.IO) {
                try {
                    if (captured) cleanPhoto(file)?.let { Draft(it, Instant.now()) } else null
                } finally {
                    file.delete()
                }
            }
            if (captured && draft == null) model.message("Could not open the photo.")
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        cameraFile = savedInstanceState?.getString("cameraFile")?.let(::File)
        setContent {
            MaterialTheme(colorScheme = lightColorScheme(primary = Color(0xFF1C6347), surface = Color(0xFFF7F7EE))) {
                val screen by model.ui.collectAsState()
                Scaffold(modifier = Modifier.safeDrawingPadding()) { padding ->
                    OperatorHome(screen, Modifier.padding(padding), ::signIn, ::toggle, ::openCamera,
                        { editSettings = true }, { model.scope.launch { model.signOut() } },
                        { model.scope.launch { model.refresh() } },
                        { startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(it))) })
                }
                if (editSettings) {
                    val registration = screen.state.registration
                    val initial = screen.state.profile?.settings ?: Settings(
                        name = registration?.name.orEmpty(), cuisine = registration?.cuisine.orEmpty(),
                    )
                    SettingsDialog(initial, screen,
                        dismiss = { editSettings = false },
                        save = { model.scope.launch { if (model.save(it)) editSettings = false } })
                }
                draft?.let { photo ->
                    MenuDialog(photo, screen, cancel = { draft = null }, publish = {
                        model.scope.launch {
                            if (model.publish(photo.jpeg, photo.captured)) draft = null
                        }
                    })
                }
            }
        }
        if (savedInstanceState == null) handle(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handle(intent)
    }

    override fun onSaveInstanceState(outState: Bundle) {
        outState.putString("cameraFile", cameraFile?.absolutePath)
        super.onSaveInstanceState(outState)
    }

    private fun handle(intent: Intent) {
        val action = intent.action
        model.scope.launch {
            model.refresh()
            when (action) {
                OperatorWidget.TOGGLE -> toggle()
                OperatorWidget.CAMERA -> openCamera()
            }
        }
    }

    private fun signIn() {
        model.scope.launch {
            try {
                login.launch(model.client.signInIntent())
            } catch (error: Exception) {
                model.message(error.message ?: "Could not open sign-in.")
            }
        }
    }

    private fun needsProfile(action: String): Boolean {
        val screen = model.ui.value
        if (screen.state.profile != null) return false
        when {
            !screen.signedIn -> model.message("Sign in to $action.")
            screen.state.registration?.status == TruckStatus.APPROVED -> {
                editSettings = true
                model.message("Set up your operating hours and area first.")
            }
            else -> model.message("Your truck needs approval before you can $action.")
        }
        return true
    }

    private fun toggle() {
        val screen = model.ui.value
        if (screen.starting || screen.state.session?.expired() == false) {
            model.scope.launch { model.stop() }
            return
        }
        if (needsProfile("share your location")) return
        if (!locationAllowed()) {
            val requested = mutableListOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION)
            if (android.os.Build.VERSION.SDK_INT >= 33) requested += Manifest.permission.POST_NOTIFICATIONS
            permission.launch(requested.toTypedArray())
        } else {
            model.scope.launch {
                if (screen.state.session != null) model.stop()
                model.start()
            }
        }
    }

    private fun locationAllowed(): Boolean =
        checkSelfPermission(Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED ||
            checkSelfPermission(Manifest.permission.ACCESS_COARSE_LOCATION) == PackageManager.PERMISSION_GRANTED

    private fun openCamera() {
        if (needsProfile("publish a menu")) return
        val directory = File(cacheDir, "camera").apply { mkdirs() }
        val file = File(directory, "${UUID.randomUUID()}.jpg")
        cameraFile = file
        try {
            camera.launch(FileProvider.getUriForFile(this, "${packageName}.camera", file))
        } catch (_: ActivityNotFoundException) {
            cameraFile = null
            model.message("Open Rolling Local on a device with a camera.")
        }
    }
}

private fun cleanPhoto(file: File): ByteArray? {
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeFile(file.absolutePath, bounds)
    val options = BitmapFactory.Options()
    while (maxOf(bounds.outWidth, bounds.outHeight) / options.inSampleSize.coerceAtLeast(1) > 2300) {
        options.inSampleSize = options.inSampleSize.coerceAtLeast(1) * 2
    }
    val bitmap = BitmapFactory.decodeFile(file.absolutePath, options) ?: return null
    val orientation = ExifInterface(file.absolutePath)
        .getAttributeInt(ExifInterface.TAG_ORIENTATION, ExifInterface.ORIENTATION_NORMAL)
    val degrees = when (orientation) {
        ExifInterface.ORIENTATION_ROTATE_90 -> 90f
        ExifInterface.ORIENTATION_ROTATE_180 -> 180f
        ExifInterface.ORIENTATION_ROTATE_270 -> 270f
        else -> 0f
    }
    val clear = if (degrees == 0f) bitmap else Bitmap.createBitmap(
        bitmap, 0, 0, bitmap.width, bitmap.height, Matrix().apply { postRotate(degrees) }, true,
    )
    return ByteArrayOutputStream().use { stream ->
        if (clear.compress(Bitmap.CompressFormat.JPEG, 85, stream)) stream.toByteArray() else null
    }
}

private fun displayTime(value: String): String =
    Instant.parse(value).atZone(ZoneId.systemDefault()).format(DateTimeFormatter.ofPattern("MMM d, h:mm a"))

@Composable
private fun OperatorHome(
    screen: OperatorScreen,
    modifier: Modifier,
    signIn: () -> Unit,
    toggle: () -> Unit,
    camera: () -> Unit,
    settings: () -> Unit,
    signOut: () -> Unit,
    refresh: () -> Unit,
    showMenu: (String) -> Unit,
) {
    val profile = screen.state.profile
    val registration = screen.state.registration
    val session = screen.state.session?.takeUnless { it.expired() }
    Column(modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(18.dp)) {
        Text(registration?.name ?: "Rolling Local", style = MaterialTheme.typography.headlineLarge)
        when {
            !screen.signedIn -> {
                Text("Share your truck’s location and current menu with nearby customers.")
                Button(signIn, enabled = !screen.busy) { Text("Sign in with Ahara") }
            }
            profile == null -> {
                when (registration?.status) {
                    TruckStatus.APPROVED -> {
                        Text("Set your weekly hours and operating area before sharing from the app.")
                        Button(settings) { Text("Set up operating hours and area") }
                    }
                    TruckStatus.PENDING -> {
                        Text("Your truck is awaiting review.")
                        OutlinedButton(refresh, enabled = !screen.busy) { Text("Check status") }
                    }
                    TruckStatus.REJECTED -> Text("This registration was declined.")
                    null -> {
                        Text("Use your Rolling Local registration link to apply for a truck listing.")
                        OutlinedButton(refresh, enabled = !screen.busy) { Text("Check status") }
                    }
                }
                TextButton(signOut) { Text("Sign out") }
            }
            else -> {
                Card {
                    Column(Modifier.fillMaxWidth().padding(18.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                        Text(when {
                            screen.starting -> "Finding location"
                            session != null -> "Sharing location"
                            else -> "Offline"
                        }, style = MaterialTheme.typography.titleLarge)
                        if (session != null) {
                            Text("Last update  ${displayTime(session.publishedAt)}")
                            Text("Expires without updates  ${displayTime(session.expiresAt)}")
                        }
                        Button(toggle) { Text(if (session != null || screen.starting) "Stop sharing" else "Start sharing") }
                    }
                }
                Card {
                    Column(Modifier.fillMaxWidth().padding(18.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                        Text("Menu", style = MaterialTheme.typography.titleLarge)
                        profile.menu?.let {
                            Text("Photo taken ${displayTime(it.capturedAt)}")
                            TextButton({ showMenu(it.url) }) { Text("View published photo") }
                        }
                        OutlinedButton(camera) { Text("Open menu camera") }
                    }
                }
                OutlinedButton(settings) { Text("Operating settings") }
                TextButton(signOut) { Text("Sign out") }
            }
        }
        screen.message?.let { Text(it, color = MaterialTheme.colorScheme.primary) }
    }
}

private data class CircleInput(val latitude: String, val longitude: String, val radius: String) {
    constructor(circle: Circle) : this(circle.center.latitude.toString(), circle.center.longitude.toString(), circle.radiusMeters.toString())

    fun area(): Circle? {
        val lat = latitude.toDoubleOrNull() ?: return null
        val lon = longitude.toDoubleOrNull() ?: return null
        val meters = radius.toDoubleOrNull() ?: return null
        if (lat !in -90.0..90.0 || lon !in -180.0..180.0 || !meters.isFinite() || meters <= 0) return null
        return Circle(Position(lat, lon), meters)
    }
}

@Composable
private fun SettingsDialog(initial: Settings, screen: OperatorScreen, dismiss: () -> Unit, save: (Settings) -> Unit) {
    var name by remember { mutableStateOf(initial.name) }
    var cuisine by remember { mutableStateOf(initial.cuisine) }
    var zone by remember { mutableStateOf(initial.timezone) }
    var opens by remember { mutableStateOf(initial.hours.opens) }
    var closes by remember { mutableStateOf(initial.hours.closes) }
    var weekdays by remember { mutableStateOf(initial.hours.weekdays.toSet()) }
    var normal by remember { mutableStateOf(CircleInput(initial.area)) }
    val activeEvent = initial.event?.takeIf { Instant.parse(it.expiresAt) > Instant.now() }
    var eventEnabled by remember { mutableStateOf(activeEvent != null) }
    var event by remember { mutableStateOf(CircleInput(activeEvent?.area ?: initial.area)) }
    var expires by remember { mutableStateOf(activeEvent?.let { Instant.parse(it.expiresAt) } ?: Instant.now().plusSeconds(7200)) }
    var issue by remember { mutableStateOf<String?>(null) }
    val context = LocalContext.current
    AlertDialog(onDismissRequest = dismiss, title = { Text("Truck settings") },
        text = {
            Column(Modifier.heightIn(max = 540.dp).verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Field("Truck name", name) { name = it }
                Field("Cuisine", cuisine) { cuisine = it }
                Field("Time zone (e.g. America/Phoenix)", zone) { zone = it }
                Field("Opens (HH:mm)", opens) { opens = it }
                Field("Closes (HH:mm)", closes) { closes = it }
                listOf(listOf("Mon", "Tue", "Wed", "Thu"), listOf("Fri", "Sat", "Sun")).forEachIndexed { row, days ->
                    Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        days.forEachIndexed { col, day ->
                            val id = row * 4 + col + 1
                            FilterChip(id in weekdays, { weekdays = if (id in weekdays) weekdays - id else weekdays + id },
                                label = { Text(day) })
                        }
                    }
                }
                Text("Normal operating circle", style = MaterialTheme.typography.titleMedium)
                CircleEditor(normal) { normal = it }
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Switch(eventEnabled, { eventEnabled = it })
                    Text("Temporary event")
                }
                if (eventEnabled) {
                    CircleEditor(event) { event = it }
                    OutlinedButton({
                        val local = expires.atZone(ZoneId.systemDefault())
                        DatePickerDialog(context, { _, year, month, day ->
                            TimePickerDialog(context, { _, hour, minute ->
                                expires = LocalDate.of(year, month + 1, day).atTime(hour, minute)
                                    .atZone(ZoneId.systemDefault()).toInstant()
                            }, local.hour, local.minute, false).show()
                        }, local.year, local.monthValue - 1, local.dayOfMonth).show()
                    }) { Text("Ends ${displayTime(expires.toString())}") }
                }
                (issue ?: screen.message)?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
        },
        confirmButton = {
            TextButton({
                val area = normal.area()
                val eventArea = if (eventEnabled) event.area() else null
                val opening = runCatching { LocalTime.parse(opens) }.getOrNull()
                val closing = runCatching { LocalTime.parse(closes) }.getOrNull()
                if (area == null || opening == null || closing == null || opening == closing ||
                    weekdays.isEmpty() || (eventEnabled && (eventArea == null || expires <= Instant.now()))) {
                    issue = "Check your hours, selected days, and operating circles."
                    return@TextButton
                }
                issue = null
                val clock = DateTimeFormatter.ofPattern("HH:mm:ss")
                save(Settings(name.trim(), cuisine.trim(), zone.trim(), Hours(weekdays.sorted(), opening.format(clock),
                    closing.format(clock)), area, eventArea?.let { EventArea(it, expires.toString()) }))
            }, enabled = !screen.busy) { Text("Save") }
        },
        dismissButton = { TextButton(dismiss) { Text("Cancel") } })
}

@Composable
private fun CircleEditor(value: CircleInput, change: (CircleInput) -> Unit) {
    Field("Center latitude", value.latitude) { change(value.copy(latitude = it)) }
    Field("Center longitude", value.longitude) { change(value.copy(longitude = it)) }
    Field("Radius in meters", value.radius) { change(value.copy(radius = it)) }
}

@Composable
private fun Field(label: String, value: String, edit: (String) -> Unit) {
    OutlinedTextField(value, edit, Modifier.fillMaxWidth(), label = { Text(label) }, singleLine = true)
}

@Composable
private fun MenuDialog(photo: Draft, screen: OperatorScreen, cancel: () -> Unit, publish: () -> Unit) {
    val bitmap = remember(photo) { BitmapFactory.decodeByteArray(photo.jpeg, 0, photo.jpeg.size)?.asImageBitmap() }
    AlertDialog(onDismissRequest = cancel, title = { Text("Menu preview") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                bitmap?.let { Image(it, "Captured menu", Modifier.fillMaxWidth().heightIn(max = 300.dp)) }
                Text(displayTime(photo.captured.toString()))
                screen.message?.let { Text(it) }
            }
        },
        confirmButton = { TextButton(publish, enabled = !screen.busy) { Text("Publish") } },
        dismissButton = { TextButton(cancel) { Text("Cancel") } })
}
