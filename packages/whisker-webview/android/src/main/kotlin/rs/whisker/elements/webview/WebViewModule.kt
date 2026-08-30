// `whisker-webview` ModuleDefinition (Android).
//
// KSP scans this module's sources for `@WhiskerModule`
// and emits the registration block into
// `WhiskerWebViewBehaviors.registerAll()`.
//
// The `WhiskerWebView` Whisker module view this references lives in
// `WhiskerWebView.kt`. Matching iOS files live under
// `packages/whisker-webview/ios/Sources/WhiskerWebView/`.
//
// Tag:  whisker-webview:WebView   (Name below + crate prefix from KSP arg)
// View: WhiskerWebView            (wraps android.webkit.WebView)

package rs.whisker.elements.webview

import rs.whisker.runtime.Module
import rs.whisker.runtime.WhiskerModule
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerValue


@WhiskerModule
class WebViewModule : Module() {
    override fun definition() = ModuleDefinition {
        Name("WebView")
        View("whisker-webview:WebView", WhiskerWebView::class.java) {

            // ---- content props -------------------------------------------

            Prop("url") { view: WhiskerWebView, value ->
                view.setUrl(value.asString() ?: "")
            }

            // Inline HTML, used only when `url` is empty.
            Prop("html") { view: WhiskerWebView, value ->
                view.setHtml(value.asString() ?: "")
            }

            // ---- behaviour props -----------------------------------------

            // Must be set before any load to take effect.
            Prop("user-agent") { view: WhiskerWebView, value ->
                view.setUserAgent(value.asString() ?: "")
            }

            // Defaults to false on Android, unlike iOS.
            Prop("javascript-enabled") { view: WhiskerWebView, value ->
                view.setJavascriptEnabled(value.asString() ?: "false")
            }

            Prop("scroll-enabled") { view: WhiskerWebView, value ->
                view.setScrollEnabled(value.asString() ?: "true")
            }

            // JSON array string, e.g. `["https://*"]`.
            Prop("origin-whitelist") { view: WhiskerWebView, value ->
                view.setOriginWhitelist(value.asString() ?: "")
            }

            // `style` is handled by the WhiskerUI base.

            // Declaration-only, but the KSP-generated registrar needs it to
            // register these names with the Host event registry; dispatch
            // itself happens inside WhiskerWebView.
            Events(
                "message",
                "load_start",
                "load",
                "navigation",
                "error",
                "progress",
            )

            // ---- one-way View commands ----------------------------------

            Command("reload") { view: WhiskerWebView, _ ->
                view.reload()
            }

            Command("goBack") { view: WhiskerWebView, _ ->
                view.goBack()
            }

            Command("goForward") { view: WhiskerWebView, _ ->
                view.goForward()
            }

            Command("stopLoading") { view: WhiskerWebView, _ ->
                view.stopLoading()
            }

            Command("postMessage") { view: WhiskerWebView, parameters ->
                val data = parameters.asString() ?: ""
                view.postMessageToPage(data)
            }

            Command("evaluateJavaScript") { view: WhiskerWebView, parameters ->
                val script = parameters.asString() ?: ""
                view.evaluateJs(script)
            }
        }
    }
}
