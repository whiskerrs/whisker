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

            // `url` — if non-empty, webView.loadUrl(). Prop is also the
            // controlled-load trigger; the view guards re-entrant loads.
            Prop("url") { view: WhiskerWebView, value ->
                view.setUrl(value.asString() ?: "")
            }

            // `html` — inline HTML, used when `url` is empty.
            Prop("html") { view: WhiskerWebView, value ->
                view.setHtml(value.asString() ?: "")
            }

            // ---- behaviour props -----------------------------------------

            // `user-agent` — overrides the WebView's user-agent string.
            Prop("user-agent") { view: WhiskerWebView, value ->
                view.setUserAgent(value.asString() ?: "")
            }

            // `javascript-enabled` — "true"/"false" string; default false
            // on Android. Must be set before any load to take effect.
            Prop("javascript-enabled") { view: WhiskerWebView, value ->
                view.setJavascriptEnabled(value.asString() ?: "false")
            }

            // `scroll-enabled` — "true"/"false". Disabling suppresses both
            // the scroll bars and touch-driven scroll/fling.
            Prop("scroll-enabled") { view: WhiskerWebView, value ->
                view.setScrollEnabled(value.asString() ?: "true")
            }

            // `origin-whitelist` — JSON array string, e.g. `["https://*"]`.
            // Parsed in the view; controls shouldOverrideUrlLoading.
            Prop("origin-whitelist") { view: WhiskerWebView, value ->
                view.setOriginWhitelist(value.asString() ?: "")
            }

            // `style` handled by the WhiskerUI base (box / layout CSS).

            // ---- events (declaration-only metadata) ----------------------
            // Actual dispatch goes through WhiskerCustomEvent.dispatch()
            // inside WhiskerWebView. This block documents the emittable set
            // so the KSP-generated registrar registers the event names with
            // Lynx's event system — matching the Rust on_* handler suffixes.
            Events(
                "message",
                "load_start",
                "load",
                "navigation",
                "error",
                "progress",
            )

            // ---- callable UI methods -------------------------------------

            // Simple navigation controls — all return Null (fire-and-forget).

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

            // `postMessage` — Rust → JS. Args: `["<string>"]` (positional).
            Function("postMessage") { view: WhiskerWebView, args ->
                val data = args.getOrNull(0)?.asString() ?: ""
                view.postMessageToPage(data)
                WhiskerValue.Null
            }

            // `evaluateJavaScript` — run script. Has two call shapes:
            //   - Fire-and-forget: Rust sends args `["<js>"]` via
            //     invoke() and ignores the return.
            //   - Result: Rust sends args via invoke_typed and awaits
            //     `{ "value": "<result>" }`.
            // Both cases share this single Function; the view's
            // evaluateJs() returns the result map (the bridge discards it
            // for fire-and-forget calls).
            //
            // NOTE: per repo memory, Android result-returning element
            // methods require invoke_async wiring in lynx_native_renderer.cc
            // (iOS-only in Lynx 3.8.0-whisker.1). Implement correctly now
            // so the method is available once the fork wires it on Android.
            Function("evaluateJavaScript") { view: WhiskerWebView, args ->
                val script = args.getOrNull(0)?.asString() ?: ""
                view.evaluateJs(script)
            }

            // `canGoBack` / `canGoForward` — sync boolean queries.
            // Returns WhiskerValue.Bool so Rust can deserialize via
            // invoke_typed::<bool>.
            Function("canGoBack") { view: WhiskerWebView, _ ->
                view.queryCanGoBack()
            }

            Function("canGoForward") { view: WhiskerWebView, _ ->
                view.queryCanGoForward()
            }
        }
    }
}
