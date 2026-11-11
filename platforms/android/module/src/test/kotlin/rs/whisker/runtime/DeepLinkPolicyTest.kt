package rs.whisker.runtime

import org.junit.Assert.*
import org.junit.Test

class DeepLinkPolicyTest {
    @Test fun acceptsOnlyViewIntentsForRegisteredSchemes() {
        val registered = setOf("myapp")
        assertTrue(DeepLinkPolicy.accepts("android.intent.action.VIEW", "MYAPP", registered))
        assertFalse(DeepLinkPolicy.accepts("android.intent.action.SEND", "myapp", registered))
        assertFalse(DeepLinkPolicy.accepts(null, "myapp", registered))
        assertFalse(DeepLinkPolicy.accepts("android.intent.action.VIEW", "https", registered))
        assertFalse(DeepLinkPolicy.accepts("android.intent.action.VIEW", null, registered))
        assertFalse(DeepLinkPolicy.accepts("android.intent.action.VIEW", "myapp", emptySet()))
    }
}
