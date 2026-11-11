package rs.whisker.runtime

import java.util.Locale

internal object DeepLinkPolicy {
    fun accepts(action: String?, scheme: String?, registered: Set<String>): Boolean =
        action == "android.intent.action.VIEW" && scheme != null &&
            registered.any { it.lowercase(Locale.ROOT) == scheme.lowercase(Locale.ROOT) }
}
