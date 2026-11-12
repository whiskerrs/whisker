package rs.whisker.modules.webbrowser

import java.net.URI
import java.net.URLDecoder
import java.util.Locale

internal object AuthRedirectPolicy {
    private fun parse(value: String): URI? = try {
        URI(value).takeIf {
            it.scheme != null && !it.isOpaque && it.rawUserInfo == null &&
                (it.rawAuthority == null || it.host != null) && it.port in -1..65535 &&
                value.none { character -> character <= ' ' || character == '\\' }
        }
    } catch (_: Exception) { null }

    private fun port(uri: URI): Int = if (uri.port >= 0) uri.port else when (uri.scheme.lowercase(Locale.ROOT)) {
        "https" -> 443
        "http" -> 80
        else -> -1
    }

    private fun query(value: String?): Map<String, List<String>> = value?.split('&')?.map { part ->
        val pieces = part.split('=', limit = 2)
        URLDecoder.decode(pieces[0], "UTF-8") to URLDecoder.decode(pieces.getOrElse(1) { "" }, "UTF-8")
    }?.groupBy({ it.first }, { it.second }) ?: emptyMap()

    fun matches(expected: String, received: String): Boolean = try {
        val redirect = parse(expected)
        val response = parse(received)
        redirect != null && response != null && redirect.rawFragment == null &&
            redirect.scheme.equals(response.scheme, ignoreCase = true) &&
            redirect.host?.lowercase(Locale.ROOT) == response.host?.lowercase(Locale.ROOT) &&
            port(redirect) == port(response) && redirect.rawPath == response.rawPath &&
            query(redirect.rawQuery).all { (key, values) -> query(response.rawQuery)[key] == values }
    } catch (_: Exception) { false }
}
