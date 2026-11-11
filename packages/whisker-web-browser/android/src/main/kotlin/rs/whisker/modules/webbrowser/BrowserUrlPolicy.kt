package rs.whisker.modules.webbrowser
import java.net.URI
internal object BrowserUrlPolicy {
    fun accepts(value: String): Boolean = try {
        val uri = URI(value)
        (uri.scheme.equals("http", true) || uri.scheme.equals("https", true)) && !uri.host.isNullOrEmpty()
    } catch (_: Exception) { false }
}
