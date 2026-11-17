package expo.modules.riftviewtls

import android.util.Base64
import expo.modules.kotlin.exception.CodedException
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import okhttp3.MediaType.Companion.toMediaTypeOrNull
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.security.MessageDigest
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import java.util.concurrent.TimeUnit
import javax.net.ssl.SSLContext
import javax.net.ssl.X509TrustManager

// See modules/riftview-tls/index.ts and client_reqs.md §4.

class PinMismatchException :
  CodedException("ERR_PIN_MISMATCH", "The host's certificate doesn't match the one saved at pairing. Its identity changed.", null)

class RequestFailedException(cause: Throwable?) :
  CodedException("ERR_REQUEST_FAILED", cause?.localizedMessage ?: "Couldn't reach the host.", cause)

/**
 * expected == null: probe mode, accept any certificate and record its hash.
 * expected != null: accept only that hash. Replaces CA and hostname validation.
 */
private class PinningTrustManager(private val expected: ByteArray?) : X509TrustManager {
  @Volatile var seenPin: ByteArray? = null
  @Volatile var pinMismatch = false

  override fun checkClientTrusted(chain: Array<X509Certificate>, authType: String) {
    throw CertificateException("Client certificates aren't used")
  }

  override fun checkServerTrusted(chain: Array<X509Certificate>, authType: String) {
    val leaf = chain.firstOrNull() ?: throw CertificateException("No certificate")
    // getEncoded() on the public key is already the SubjectPublicKeyInfo DER.
    val pin = MessageDigest.getInstance("SHA-256").digest(leaf.publicKey.encoded)
    seenPin = pin
    if (expected != null && !MessageDigest.isEqual(expected, pin)) {
      pinMismatch = true
      throw CertificateException("Certificate pin mismatch")
    }
  }

  override fun getAcceptedIssuers(): Array<X509Certificate> = arrayOf()
}

private fun clientFor(trustManager: PinningTrustManager, timeoutMs: Long): OkHttpClient {
  val ssl = SSLContext.getInstance("TLS").apply { init(null, arrayOf(trustManager), null) }
  return OkHttpClient.Builder()
    .sslSocketFactory(ssl.socketFactory, trustManager)
    // We connect by IP and the cert names riftview.local; the pin is the identity check.
    .hostnameVerifier { _, _ -> true }
    .connectTimeout(timeoutMs, TimeUnit.MILLISECONDS)
    .readTimeout(timeoutMs, TimeUnit.MILLISECONDS)
    .callTimeout(timeoutMs, TimeUnit.MILLISECONDS)
    .cache(null)
    .build()
}

class RiftviewTlsModule : Module() {
  override fun definition() = ModuleDefinition {
    Name("RiftviewTls")

    // Async functions run off the main thread, so blocking execute() is fine.
    AsyncFunction("probePin") { host: String, port: Int, timeoutMs: Double ->
      val trustManager = PinningTrustManager(expected = null)
      val client = clientFor(trustManager, timeoutMs.toLong())
      val request = Request.Builder().url("https://$host:$port/api/health").build()
      var failure: Throwable? = null
      try {
        client.newCall(request).execute().close()
      } catch (e: Exception) {
        failure = e
      } finally {
        client.connectionPool.evictAll()
      }
      // Captured during the handshake, so it's available even if HTTP failed afterwards.
      val pin = trustManager.seenPin ?: throw RequestFailedException(failure)
      Base64.encodeToString(pin, Base64.NO_WRAP)
    }

    AsyncFunction("request") { method: String, url: String, headers: Map<String, String>, body: String?, pin: String, timeoutMs: Double ->
      val expected = try {
        Base64.decode(pin, Base64.DEFAULT)
      } catch (e: IllegalArgumentException) {
        throw RequestFailedException(e)
      }
      if (expected.size != 32) throw RequestFailedException(null)

      val trustManager = PinningTrustManager(expected)
      val client = clientFor(trustManager, timeoutMs.toLong())
      val contentType = headers.entries.firstOrNull { it.key.equals("content-type", true) }?.value
      val requestBody = body?.toRequestBody(contentType?.toMediaTypeOrNull())
      val builder = Request.Builder().url(url).method(method, requestBody)
      headers.forEach { (name, value) -> builder.header(name, value) }

      try {
        client.newCall(builder.build()).execute().use { response ->
          mapOf(
            "status" to response.code,
            "headers" to response.headers.associate { (name, value) -> name.lowercase() to value },
            "body" to (response.body?.string() ?: "")
          )
        }
      } catch (e: Exception) {
        if (trustManager.pinMismatch) throw PinMismatchException()
        throw RequestFailedException(e)
      } finally {
        client.connectionPool.evictAll()
      }
    }
  }
}
