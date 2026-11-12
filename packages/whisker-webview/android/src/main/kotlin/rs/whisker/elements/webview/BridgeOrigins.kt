package rs.whisker.elements.webview

import java.net.URI
import java.util.Locale

internal object BridgeOrigins {
    fun canonical(value: String): String? = try {
        val uri = URI(value)
        val scheme = uri.scheme?.lowercase(Locale.ROOT)
        val host = uri.host?.lowercase(Locale.ROOT)
        if (scheme !in setOf("http", "https") || host.isNullOrEmpty() || host.endsWith(".") ||
            uri.rawUserInfo != null || uri.rawQuery != null || uri.rawFragment != null ||
            uri.rawPath !in listOf("", "/") || uri.port !in -1..65535 ||
            value.any { it <= ' ' || it == '\\' || it == '*' || it.code >= 127 }) null
        else "$scheme://$host:${if (uri.port >= 0) uri.port else if (scheme == "https") 443 else 80}"
    } catch (_: Exception) { null }

    fun permits(value: String, mainFrame: Boolean, origins: Set<String>): Boolean =
        mainFrame && canonical(value)?.let { it in origins } == true
}
