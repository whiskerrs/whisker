package rs.whisker.elements.webview

import java.net.URI
import java.util.Locale

internal object OriginPolicy {
    private fun parse(value: String): URI? = try {
        if (value.any { it <= ' ' || it == '\\' || it.code >= 127 }) null
        else URI(value).takeIf {
            it.scheme?.lowercase(Locale.ROOT) in setOf("http", "https") &&
                !it.host.isNullOrEmpty() && it.rawUserInfo == null &&
                !it.host.endsWith(".") && it.port in -1..65535
        }
    } catch (_: Exception) { null }

    private fun port(uri: URI): Int = if (uri.port >= 0) uri.port
        else if (uri.scheme.equals("https", ignoreCase = true)) 443 else 80

    fun matches(pattern: String, url: String): Boolean {
        val candidate = parse(url) ?: return false
        val wildcard = pattern.contains("://*")
        val expanded = pattern.replace("://*.", "://whisker-wildcard.")
            .replace("://*", "://whisker-any")
        if ('*' in expanded) return false
        val rule = parse(expanded) ?: return false
        if (rule.rawPath !in listOf("", "/") || rule.rawQuery != null || rule.rawFragment != null) return false
        if (!candidate.scheme.equals(rule.scheme, ignoreCase = true) || port(candidate) != port(rule)) return false
        val host = candidate.host.lowercase(Locale.ROOT)
        val allowed = rule.host.lowercase(Locale.ROOT)
        return when {
            wildcard && allowed == "whisker-any" -> true
            wildcard && allowed.startsWith("whisker-wildcard.") -> {
                val suffix = allowed.removePrefix("whisker-wildcard")
                host.endsWith(suffix) && host.length > suffix.length
            }
            wildcard -> false
            else -> host == allowed
        }
    }
}
