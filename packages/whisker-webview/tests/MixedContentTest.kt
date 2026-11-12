package rs.whisker.security

import android.test.InstrumentationTestCase
import android.webkit.WebSettings
import rs.whisker.elements.webview.WhiskerWebView

/** Verify the production WebView denies cleartext subresources on HTTPS pages. */
class MixedContentTest : InstrumentationTestCase() {
    fun testMixedContentIsNeverAllowed() {
        instrumentation.runOnMainSync {
            val view = WhiskerWebView(instrumentation.targetContext)
            assertEquals(WebSettings.MIXED_CONTENT_NEVER_ALLOW, view.view().settings.mixedContentMode)
            view.view().destroy()
        }
    }
}
