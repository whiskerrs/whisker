// `whisker-webview` ModuleDefinition (Android).
//
// KSP scans this module's sources for any concrete `Module` subclass
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
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerValue

class WebViewModule : Module() {
    override fun definition() = ModuleDefinition {
        Name("WebView")
        View(WhiskerWebView::class.java) {

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

            // One Function serves both `invoke` (ignores the return) and
            // `invoke_typed` (awaits `{ "value": "<result>" }`).
            //
            // TODO: Android result-returning element methods need
            // invoke_async wiring in lynx_native_renderer.cc, compiled
            // iOS-only in Lynx 3.8.0-whisker.1 (memory note
            // `whisker_element_method_results_need_async`).
            Function("evaluateJavaScript") { view: WhiskerWebView, args ->
                val script = args.getOrNull(0)?.asString() ?: ""
                view.evaluateJs(script)
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
