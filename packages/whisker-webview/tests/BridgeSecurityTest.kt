package rs.whisker.security

import android.test.InstrumentationTestCase
import rs.whisker.elements.webview.WhiskerWebView
import rs.whisker.runtime.WhiskerValue
import java.util.concurrent.ConcurrentLinkedQueue

class BridgeSecurityTest : InstrumentationTestCase() {
    fun testOnlyAllowedMainFrameCanSendMessages() {
        val received = ConcurrentLinkedQueue<String>()
        lateinit var view: WhiskerWebView
        instrumentation.runOnMainSync {
            view = WhiskerWebView(instrumentation.targetContext)
            view.installWhiskerEventSink { name, detail ->
                if (name == "message") {
                    val data = (detail as? WhiskerValue.Map)?.value?.get("data")?.asString()
                    if (data != null) received.add(data)
                }
            }
            view.setJavascriptEnabled(true)
            view.setMessageOrigins("[\"https://allowed.example\"]")
            view.view().loadDataWithBaseURL("https://allowed.example", """
                <html><body><script>setTimeout(function(){window.whisker.postMessage('main');},200);</script>
                <iframe srcdoc="<script>setTimeout(function(){window.whisker.postMessage('iframe');},200);</script>"></iframe>
                </body></html>
            """.trimIndent(), "text/html", "UTF-8", null)
        }
        val deadline = System.currentTimeMillis() + 10000
        while (received.isEmpty() && System.currentTimeMillis() < deadline) Thread.sleep(100)
        Thread.sleep(700)
        assertEquals(listOf("main"), received.toList())
        received.clear()
        instrumentation.runOnMainSync {
            view.view().loadDataWithBaseURL("https://evil.example", "<script>setTimeout(function(){window.whisker.postMessage('evil');},200);</script>", "text/html", "UTF-8", null)
        }
        Thread.sleep(1500)
        assertTrue(received.isEmpty())
        instrumentation.runOnMainSync { view.view().destroy() }
    }
}
