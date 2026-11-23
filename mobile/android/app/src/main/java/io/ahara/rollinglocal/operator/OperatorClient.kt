package io.ahara.rollinglocal.operator

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import net.openid.appauth.AuthState
import net.openid.appauth.AuthorizationException
import net.openid.appauth.AuthorizationRequest
import net.openid.appauth.AuthorizationResponse
import net.openid.appauth.AuthorizationService
import net.openid.appauth.AuthorizationServiceConfiguration
import net.openid.appauth.ResponseTypeValues
import java.net.HttpURLConnection
import java.net.URI
import java.security.KeyStore
import java.time.Instant
import java.time.ZoneId
import java.util.UUID
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import kotlin.coroutines.resume
import kotlin.coroutines.suspendCoroutine

@Serializable data class Position(val latitude: Double, val longitude: Double)
@Serializable data class Circle(val center: Position, val radiusMeters: Double)
@Serializable data class EventArea(val area: Circle, val expiresAt: String)
@Serializable data class Hours(val weekdays: List<Int>, val opens: String, val closes: String)

@Serializable data class Settings(
    val name: String = "",
    val cuisine: String = "",
    val timezone: String = ZoneId.systemDefault().id,
    val hours: Hours = Hours(listOf(2, 3, 4, 5, 6), "11:00:00", "15:00:00"),
    val area: Circle = Circle(Position(34.540, -112.468), 8_000.0),
    val event: EventArea? = null,
)

@Serializable data class Menu(val id: String, val url: String, val capturedAt: String, val publishedAt: String)
@Serializable data class MenuItem(val id: String = UUID.randomUUID().toString(), val position: Int = 0,
    val name: String = "", val description: String? = null, val priceText: String? = null, val tagIds: List<String> = emptyList())
@Serializable data class MenuTag(val id: String, val label: String)
@Serializable data class MenuExtraction(val menuId: String, val revision: String, val status: String, val items: List<MenuItem>)
@Serializable data class MenuResults(val extraction: MenuExtraction?, val tags: List<MenuTag>)
@Serializable data class MenuCorrection(val menuId: String, val revision: String?, val items: List<MenuItem>)
@Serializable data class Profile(val id: String, val settings: Settings, val menu: Menu?)
@Serializable enum class TruckStatus {
    @SerialName("pending") PENDING,
    @SerialName("approved") APPROVED,
    @SerialName("rejected") REJECTED,
}
@Serializable data class Registration(val name: String, val cuisine: String, val status: TruckStatus)
@Serializable data class Sharing(val id: String, val publishedAt: String, val expiresAt: String, val closesAt: String) {
    fun expired(now: Instant = Instant.now()): Boolean = now >= Instant.parse(expiresAt)
}
@Serializable data class OperatorState(
    val profile: Profile? = null,
    val session: Sharing? = null,
    val registration: Registration? = null,
)
@Serializable data class Fix(val position: Position, val accuracyMeters: Double, val recordedAt: String)
@Serializable private data class LoginConfig(
    val issuer: String,
    val audience: String,
    val connection: String,
    val androidClientId: String,
    val redirectUri: String,
)
@Serializable private data class Start(val id: String, val fix: Fix)
@Serializable private data class Upload(val bytes: Int)
@Serializable private data class Ticket(val id: String, val url: String)
@Serializable private data class Publish(val id: String, val capturedAt: String)
@Serializable private data class Failure(val error: String)

class ServerError(val code: Int, message: String) : Exception(message)

class OperatorClient(context: Context) {
    private val service = AuthorizationService(context)
    private val vault = LoginVault(context)
    private val lock = Mutex()
    private val json = Json { ignoreUnknownKeys = true; encodeDefaults = true }
    private var auth = vault.read()?.let { runCatching { AuthState.jsonDeserialize(it) }.getOrNull() }
    val signedIn: Boolean get() = auth?.isAuthorized == true

    suspend fun signInIntent(): Intent {
        val config = request<LoginConfig>("public/config", authenticated = false)
        val domain = Uri.parse(config.issuer)
        require(domain.scheme == "https")
        val serviceConfig = AuthorizationServiceConfiguration(
            domain.buildUpon().appendPath("authorize").build(),
            domain.buildUpon().appendPath("oauth").appendPath("token").build(),
        )
        val request = AuthorizationRequest.Builder(
            serviceConfig, config.androidClientId, ResponseTypeValues.CODE, Uri.parse(config.redirectUri),
        ).setScope("openid email offline_access")
            .setAdditionalParameters(mapOf("audience" to config.audience, "connection" to config.connection))
            .build()
        return service.getAuthorizationRequestIntent(request)
    }

    suspend fun signInResult(data: Intent) = lock.withLock {
        val response = AuthorizationResponse.fromIntent(data)
            ?: throw ServerError(401, AuthorizationException.fromIntent(data)?.errorDescription ?: "Sign-in was cancelled.")
        val tokens = suspendCoroutine { continuation ->
            service.performTokenRequest(response.createTokenExchangeRequest()) { result, error ->
                continuation.resume(result to error)
            }
        }
        val token = tokens.first ?: throw ServerError(401, tokens.second?.errorDescription ?: "Ahara did not authorize this account.")
        val next = AuthState(response, token, null)
        vault.write(next.jsonSerializeString())
        auth = next
    }

    suspend fun signOut() = lock.withLock {
        auth = null
        vault.write(null)
    }

    private suspend fun token(): String = lock.withLock {
        val state = auth ?: throw ServerError(401, "Sign in to continue.")
        val response = suspendCoroutine { continuation ->
            state.performActionWithFreshTokens(service) { access, _, error ->
                continuation.resume(access to error)
            }
        }
        val access = response.first ?: throw ServerError(401, response.second?.errorDescription ?: "Sign in again to continue.")
        vault.write(state.jsonSerializeString())
        access
    }

    suspend fun state(): OperatorState = request("operator")
    suspend fun menuResults(): MenuResults = request("operator/menu/extraction")
    suspend fun retryMenu(failed: MenuExtraction): MenuResults = request("operator/menu/extraction/retry", "POST",
        json.encodeToString(mapOf("menuId" to failed.menuId, "revision" to failed.revision)))
    suspend fun saveMenu(value: MenuCorrection): MenuResults =
        request("operator/menu/extraction", "PUT", json.encodeToString(value))
    suspend fun settings(settings: Settings): OperatorState = request("operator/settings", "PUT", json.encodeToString(settings))
    suspend fun start(id: UUID, fix: Fix): Sharing = request("operator/sessions", "POST", json.encodeToString(Start(id.toString(), fix)))
    suspend fun update(id: UUID, fix: Fix): Sharing = request("operator/sessions/$id/fix", "PUT", json.encodeToString(fix))
    suspend fun stop(id: String): OperatorState = request("operator/sessions/$id", "DELETE")

    suspend fun publish(jpeg: ByteArray, capturedAt: Instant): Menu {
        val ticket = request<Ticket>("operator/menu/upload", "POST", json.encodeToString(Upload(jpeg.size)))
        val uri = URI(ticket.url)
        require(uri.scheme == "https")
        exchange(uri, "PUT", null, jpeg, "image/jpeg")
        return request("operator/menu/publish", "POST", json.encodeToString(Publish(ticket.id, capturedAt.toString())))
    }

    private suspend inline fun <reified T> request(
        path: String,
        method: String = "GET",
        payload: String? = null,
        authenticated: Boolean = true,
    ): T {
        val bearer = if (authenticated) token() else null
        return json.decodeFromString(exchange(
            URI("${BuildConfig.API_ORIGIN}/$path"), method, bearer, payload?.toByteArray(), "application/json",
        ))
    }

    private suspend fun exchange(uri: URI, method: String, bearer: String?, payload: ByteArray?, contentType: String): String =
        withContext(Dispatchers.IO) {
            val connection = uri.toURL().openConnection() as HttpURLConnection
            try {
                connection.requestMethod = method
                connection.connectTimeout = 10_000
                connection.readTimeout = if (contentType == "image/jpeg") 45_000 else 15_000
                connection.instanceFollowRedirects = false
                if (bearer != null) connection.setRequestProperty("Authorization", "Bearer $bearer")
                if (payload != null) {
                    connection.doOutput = true
                    connection.setFixedLengthStreamingMode(payload.size)
                    connection.setRequestProperty("Content-Type", contentType)
                    connection.outputStream.use { it.write(payload) }
                }
                val code = connection.responseCode
                if (code in 200..299) {
                    connection.inputStream.bufferedReader().use { it.readText() }
                } else {
                    val data = connection.errorStream?.bufferedReader()?.use { it.readText() }
                    val message = data?.let { runCatching { json.decodeFromString<Failure>(it).error }.getOrNull() }
                    throw ServerError(code, message ?: "The request could not be completed.")
                }
            } finally {
                connection.disconnect()
            }
        }
}

private class LoginVault(context: Context) {
    private val prefs = context.getSharedPreferences("auth0-login", Context.MODE_PRIVATE)
    private val alias = "rolling-local-operator"
    private val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }

    fun write(value: String?) {
        if (value == null) {
            prefs.edit().clear().apply()
            return
        }
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val authenticated = cipher.doFinal(value.toByteArray())
        prefs.edit().putString("token", Base64.encodeToString(cipher.iv + authenticated, Base64.NO_WRAP)).apply()
    }

    fun read(): String? {
        val token = prefs.getString("token", null) ?: return null
        return runCatching {
            val data = Base64.decode(token, Base64.NO_WRAP)
            require(data.size > 12)
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, data.copyOfRange(0, 12)))
            String(cipher.doFinal(data.copyOfRange(12, data.size)))
        }.getOrNull()
    }

    private fun key(): SecretKey {
        (store.getKey(alias, null) as? SecretKey)?.let { return it }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").run {
            init(KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build())
            generateKey()
        }
    }
}
