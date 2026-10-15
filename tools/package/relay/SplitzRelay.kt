package uniffi.splitz_ffi

import java.io.IOException
import java.io.InputStream
import java.net.HttpURLConnection
import java.net.URL

/// A SPEC.md §15.5 relay over HTTP: `POST <origin>/c/<channel>` with
/// `{"blobs":[…],"work":"…"}` adds blobs, `GET <origin>/c/<channel>` returns every blob
/// the channel holds.
///
/// The library decides what goes on the wire and what an answer means
/// (`relayChannelUrl`, `relayPushBody`, `relayWorkAsked`, `relayPushAnswer`,
/// `relayFetchAnswer`); this class moves the bytes with `HttpURLConnection`,
/// which every JVM and every Android API level carries. An answer is read
/// whatever its HTTP status: the body says whether the relay took a push.
///
/// Both calls block, so on Android they run off the main thread. Every
/// failure raises `SplitzException.Host`, whose `transient` says whether
/// retrying later could succeed: a relay that could not be reached, or that
/// refused or answered with something that is not a channel, is transient; an
/// origin carrying a query or a fragment, or a blob over 65536 characters, is
/// not.
class SplitzRelay(
    /// A scheme, a host and an optional path. The channel is appended to it.
    val origin: String,
    /// Applied to connecting and to reading, each.
    private val timeoutMillis: Int = 30_000,
) {
    init {
        relayChannelUrl(origin, "")
    }

    /// Adds `blobs` to `channel`. Pushing a blob the channel already holds
    /// changes nothing, so a retry cannot create a duplicate. An empty list
    /// makes no request. The push carries work (§15.5), stamped at 16 bits
    /// and once more at the base a refusal asks for, so it is slow to call.
    fun push(channel: String, blobs: List<String>) {
        val url = relayChannelUrl(origin, channel)
        val body = relayPushBody(channel, blobs, 16u) ?: return
        val answer = exchange("POST", url, body)
        val asked = relayWorkAsked(answer, 16u)
        if (asked == null) {
            relayPushAnswer(answer)
            return
        }
        relayPushAnswer(exchange("POST", url, relayPushBody(channel, blobs, asked)!!))
    }

    /// Every blob `channel` currently holds, including ones the caller
    /// already has; merging them is idempotent.
    fun fetch(channel: String): List<String> =
        relayFetchAnswer(exchange("GET", relayChannelUrl(origin, channel), null))

    private fun exchange(method: String, url: String, body: String?): String {
        var connection: HttpURLConnection? = null
        try {
            connection = URL(url).openConnection() as? HttpURLConnection
                ?: throw IOException("$url is not an HTTP URL")
            connection.requestMethod = method
            connection.connectTimeout = timeoutMillis
            connection.readTimeout = timeoutMillis
            if (body != null) {
                val bytes = body.toByteArray(Charsets.UTF_8)
                connection.doOutput = true
                connection.setRequestProperty("Content-Type", "application/json; charset=utf-8")
                connection.setFixedLengthStreamingMode(bytes.size)
                connection.outputStream.use { it.write(bytes) }
            }
            val stream: InputStream? =
                if (connection.responseCode >= 400) connection.errorStream else connection.inputStream
            return stream?.use { String(it.readBytes(), Charsets.UTF_8) } ?: ""
        } catch (e: IOException) {
            throw SplitzException.Host("Could not reach the relay: $e", true)
        } finally {
            connection?.disconnect()
        }
    }
}
