// `whisker-webview` ModuleDefinition (Android).
//
// KSP scans this module's sources for `@WhiskerModule`
// and emits the registration block into
// `WhiskerWebViewBehaviors.registerAll()`.
//
// The `WhiskerWebView` Lynx UI subclass this references lives in
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
            // register these names with Lynx's event system; dispatch
            // itself happens inside WhiskerWebView.
            Events(
                "message",
                "load_start",
                "load",
                "navigation",
                "error",
                "progress",
            )

            // ---- callable UI methods -------------------------------------

            Function("reload") { view: WhiskerWebView, _ ->
                view.reload()
                WhiskerValue.Null
            }

            Function("goBack") { view: WhiskerWebView, _ ->
                view.goBack()
                WhiskerValue.Null
            }

            Function("goForward") { view: WhiskerWebView, _ ->
                view.goForward()
                WhiskerValue.Null
            }

            Function("stopLoading") { view: WhiskerWebView, _ ->
                view.stopLoading()
                WhiskerValue.Null
            }

            Function("postMessage") { view: WhiskerWebView, args ->
                val data = args.getOrNull(0)?.asString() ?: ""
                view.postMessageToPage(data)
                WhiskerValue.Null
            }

            // Fire-and-forget; the result-returning form is the
            // AsyncFunction below.
            Function("evaluateJavaScript") { view: WhiskerWebView, args ->
                val script = args.getOrNull(0)?.asString() ?: ""
                view.evaluateJs(script)
            }

            // Async so the WebView's ValueCallback can carry the JS
            // result back through the promise.
            AsyncFunction("evaluateJavaScriptWithResult") { view: WhiskerWebView, args, promise ->
                val script = args.getOrNull(0)?.asString()
                if (script == null) {
                    promise.reject("evaluateJavaScriptWithResult: missing script argument")
                } else {
                    view.evaluateJsWithResult(script, promise)
                }
            }

            Function("canGoBack") { view: WhiskerWebView, _ ->
                view.queryCanGoBack()
            }

            Function("canGoForward") { view: WhiskerWebView, _ ->
                view.queryCanGoForward()
            }
        }
    }
}
