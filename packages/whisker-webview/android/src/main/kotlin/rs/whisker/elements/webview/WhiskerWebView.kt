// Lynx UI subclass hosting a native android.webkit.WebView.
// A plain `WhiskerUI` subclass — no Whisker annotations; registration
// is driven by `WebViewModule`'s `definition()` (see `WebViewModule.kt`).
//
// ## Content loading
//
// `url` and `html` are mutually exclusive: when `url` is non-empty it
// takes priority. A guard (`lastLoadedUrl`) prevents echoing internal
// navigations (redirects, pushState) back as a new loadUrl() call — the
// prop settles on the URL passed DOWN from Rust; the native WebView's
// own navigation is not written back up (one-way-down controlled prop).
//
// ## JS bridge
//
// `window.whisker` is injected at document start via
// WebViewCompat.addDocumentStartJavaScript (requires API 24+ in the
// compat lib; the compat call is wrapped in a static-method existence
// check and falls back gracefully to onPageStarted injection on older
// builds). The shim wires:
//   - page → Rust: window.whisker.postMessage(data) → JavascriptInterface
//     → emitEvent("message", …).
//   - Rust → page: evaluateJavascript("window.whisker._receive(…)") or
//     arbitrary JS via evaluateJs().
//
// ## Event dispatch
//
// WebViewClient / WebChromeClient callbacks fire on the UI thread and
// dispatch SYNCHRONOUSLY. They can arrive while Lynx is mid-teardown /
// hot-reload remount, but the Rust renderer is now re-entrancy-safe
// (whisker #3: `with_renderer` takes a shared borrow and every renderer
// field borrow is scoped so it never spans a re-entrant FFI call), so a
// re-entrant dispatch no longer panics with "RefCell already borrowed".
// This used to be deferred a main-loop tick via `view?.post { … }`;
// dispatching synchronously removes that one-tick lag (whisker #3).
//
// The ONE exception is `@JavascriptInterface` (window.whisker.postMessage
// → emitMessage), which fires on JavaBridge (a background thread). That
// path still hops to the UI thread via `view?.post { … }` before touching
// any Android View / Lynx emitter state — a real thread transition, not a
// reentrancy guard, so it is kept.
//
// ## Teardown
//
// An OnAttachStateChangeListener on the native WebView tears down the web
// process when the view is detached from its window: stopLoading(),
// removeJavascriptInterface(), destroy(). This releases the renderer
// process promptly and avoids leaking the WebView after Lynx removes the
// element. The listener also defers JS-shim injection to after the first
// attach (for the document-start fallback path on older APIs).
//
// ## Scroll
//
// scroll-enabled=false stores a flag and installs an OnTouchListener that
// consumes ACTION_MOVE / ACTION_UP events for SOURCE_CLASS_POINTER (finger
// scroll), while still allowing programmatic scrollTo() calls. The
// scrollbar visibility is also toggled so the disabled state is visible.

package rs.whisker.elements.webview

import android.annotation.SuppressLint
import android.content.Context
import android.os.Build
import android.webkit.JavascriptInterface
import android.webkit.WebChromeClient
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerCustomEvent
import rs.whisker.runtime.WhiskerUI
import rs.whisker.runtime.WhiskerValue

open class WhiskerWebView(context: WhiskerContext) :
    WhiskerUI<android.webkit.WebView>(context) {

    // -------------------------------------------------------------------------
    // JS shim injected at document start
    // -------------------------------------------------------------------------

    companion object {
        /**
         * Injected into every document before any page script runs.
         *
         * - `window.whisker.postMessage(data)` — page → Rust.
         *   Delegates to `__whisker_android.postMessage(s)` (the
         *   JavascriptInterface), serialising non-string args to JSON.
         * - `window.whisker._receive(data)` — Rust → page.
         *   Calls `window.whisker.onMessage` if the page has set it.
         */
        private const val JS_SHIM = """
(function() {
  if (!window.whisker) { window.whisker = {}; }
  window.whisker.postMessage = function(data) {
    var s = (typeof data === 'string') ? data : JSON.stringify(data);
    window.__whisker_android.postMessage(s);
  };
  window.whisker._receive = function(data) {
    if (typeof window.whisker.onMessage === 'function') {
      window.whisker.onMessage(data);
    }
  };
})();
"""
    }

    // -------------------------------------------------------------------------
    // State
    // -------------------------------------------------------------------------

    /** The URL most recently passed to loadUrl(). Used to suppress echo-loads:
     *  when the `url` prop arrives with the same string already loaded we
     *  skip the call so we don't interrupt an in-flight navigation or reset
     *  the browser's own history. Null means nothing has been loaded yet. */
    private var lastLoadedUrl: String? = null

    /** The current `html` prop value. Only rendered when `url` is empty. */
    private var pendingHtml: String = ""

    /** Whether JavaScript is enabled (set via the `javascript-enabled` prop). */
    private var jsEnabled: Boolean = false

    /** Whether the user can scroll the web content via touch. */
    private var scrollEnabled: Boolean = true

    /** Parsed origin whitelist (glob patterns). An empty list means "allow all". */
    private var originWhitelist: List<String> = listOf("https://*", "http://*")

    // -------------------------------------------------------------------------
    // View creation
    // -------------------------------------------------------------------------

    @SuppressLint("SetJavaScriptEnabled")
    override fun createView(context: Context): android.webkit.WebView {
        val wv = android.webkit.WebView(context)

        // Settings baseline. javaScriptEnabled starts false — the prop setter
        // enables it when the Rust side sets javascript-enabled="true".
        wv.settings.apply {
            javaScriptEnabled = false
            domStorageEnabled = true
            // Defense-in-depth: the component never loads file:// URLs (inline
            // HTML goes through loadDataWithBaseURL(null, …), URL loads are
            // http/https), so deny local-file and content:// access. Without
            // this, a file:// page (or a redirect to one) could read app-sandbox
            // or device files; allowFileAccess defaults to true below API 30.
            // (allowFileAccessFromFileURLs / allowUniversalAccessFromFileURLs are
            // deprecated and already default to false, so they're not set here.)
            allowFileAccess = false
            allowContentAccess = false
            // Allow mixed content for http:// URLs when the whitelist permits
            // them (the default whitelist includes http://*). API 21+.
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP) {
                mixedContentMode = WebSettings.MIXED_CONTENT_COMPATIBILITY_MODE
            }
        }

        // Wire the JS bridge (Rust→page evaluateJavascript, page→Rust
        // postMessage). The interface is installed before any load so it is
        // always present by the time page scripts run.
        wv.addJavascriptInterface(WhiskerBridge(), "__whisker_android")

        // Inject the window.whisker shim at document start when the compat
        // library supports it (API 24+). On older devices we fall back to
        // evaluateJavascript in onPageStarted — slightly later, but the shim
        // still runs before user scripts that depend on DOMContentLoaded.
        val shimInjectedViaCompat = if (WebViewFeature.isFeatureSupported(
                WebViewFeature.DOCUMENT_START_SCRIPT)
        ) {
            WebViewCompat.addDocumentStartJavaScript(wv, JS_SHIM, setOf("*"))
            true
        } else {
            false
        }

        // Install clients. Defined as inner objects to capture the
        // shimInjectedViaCompat flag without an extra field.
        wv.webViewClient = object : WebViewClient() {
            override fun onPageStarted(
                view: android.webkit.WebView,
                url: String?,
                favicon: android.graphics.Bitmap?,
            ) {
                // Fallback shim injection for API < 24.
                if (!shimInjectedViaCompat) {
                    view.evaluateJavascript(JS_SHIM, null)
                }
                val safeUrl = url ?: ""
                WhiskerCustomEvent.dispatch(
                    ui = this@WhiskerWebView,
                    name = "load_start",
                    params = mapOf("url" to safeUrl),
                )
            }

            override fun onPageFinished(view: android.webkit.WebView, url: String?) {
                val safeUrl = url ?: ""
                WhiskerCustomEvent.dispatch(
                    ui = this@WhiskerWebView,
                    name = "load",
                    params = mapOf("url" to safeUrl),
                )
            }

            override fun onReceivedError(
                view: android.webkit.WebView,
                request: WebResourceRequest?,
                error: WebResourceError?,
            ) {
                // Only surface main-frame errors; sub-resource errors (images,
                // fonts) would spam the Rust side with non-actionable events.
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M &&
                    request?.isForMainFrame == false) return

                val url = request?.url?.toString() ?: ""
                val code = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
                    error?.errorCode ?: -1
                } else {
                    -1
                }
                val description = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
                    error?.description?.toString() ?: ""
                } else {
                    ""
                }
                WhiskerCustomEvent.dispatch(
                    ui = this@WhiskerWebView,
                    name = "error",
                    params = mapOf(
                        "url" to url,
                        "code" to code,
                        "description" to description,
                    ),
                )
            }

            override fun shouldOverrideUrlLoading(
                view: android.webkit.WebView,
                request: WebResourceRequest?,
            ): Boolean {
                val url = request?.url?.toString() ?: return false
                return handleNavigation(view, url)
            }

            // API < 24 fallback for shouldOverrideUrlLoading.
            @Suppress("OVERRIDE_DEPRECATION")
            override fun shouldOverrideUrlLoading(
                view: android.webkit.WebView,
                url: String?,
            ): Boolean {
                val safeUrl = url ?: return false
                return handleNavigation(view, safeUrl)
            }

            /**
             * Apply origin whitelist check and emit the `navigation` event.
             *
             * Returns true  (block, WebView won't load it) when the URL is NOT
             * in the whitelist.
             * Returns false (allow) when the URL matches a whitelist pattern;
             * also emits `navigation` so Rust can observe internal link clicks.
             */
            private fun handleNavigation(
                view: android.webkit.WebView,
                url: String,
            ): Boolean {
                val allowed = originWhitelist.isEmpty() ||
                    originWhitelist.any { pattern -> matchesGlob(pattern, url) }
                if (!allowed) return true // block
                // Allowed — let WebView load it and inform Rust.
                WhiskerCustomEvent.dispatch(
                    ui = this@WhiskerWebView,
                    name = "navigation",
                    params = mapOf("url" to url),
                )
                return false // proceed with load
            }
        }

        wv.webChromeClient = object : WebChromeClient() {
            override fun onProgressChanged(view: android.webkit.WebView, newProgress: Int) {
                val fraction = newProgress / 100.0
                WhiskerCustomEvent.dispatch(
                    ui = this@WhiskerWebView,
                    name = "progress",
                    params = mapOf("progress" to fraction),
                )
            }
        }

        // Teardown hook: clean up the web process when the view is detached.
        wv.addOnAttachStateChangeListener(
            object : android.view.View.OnAttachStateChangeListener {
                override fun onViewAttachedToWindow(v: android.view.View) {}

                override fun onViewDetachedFromWindow(v: android.view.View) {
                    val webView = v as? android.webkit.WebView ?: return
                    webView.stopLoading()
                    webView.removeJavascriptInterface("__whisker_android")
                    webView.destroy()
                }
            }
        )

        return wv
    }

    // -------------------------------------------------------------------------
    // Props called from WebViewModule
    // -------------------------------------------------------------------------

    /**
     * `url` prop setter. Triggers a loadUrl() when non-empty and when the
     * incoming URL differs from what is already loaded, to prevent re-loading
     * the page on every Rust re-render that touches an unrelated prop.
     */
    fun setUrl(incoming: String) {
        val wv = view ?: return
        if (incoming.isEmpty()) return
        // Guard: don't echo internal navigations back as a re-load. If the
        // app writes the same URL signal again (e.g. reset to initial URL
        // after a back-navigation) we still load — `lastLoadedUrl` tracks
        // only the last PROP-initiated load, not internal redirects.
        if (incoming == lastLoadedUrl) return
        lastLoadedUrl = incoming
        wv.loadUrl(incoming)
    }

    /**
     * `html` prop setter. Renders inline HTML via loadDataWithBaseURL when
     * there is no `url` prop set (url="" or url not provided).
     * If a `url` is currently active we store the HTML but don't render it —
     * the url prop takes priority.
     */
    fun setHtml(html: String) {
        pendingHtml = html
        val wv = view ?: return
        // Only render HTML when url is empty / unset (url takes priority).
        if (lastLoadedUrl != null && lastLoadedUrl!!.isNotEmpty()) return
        if (html.isEmpty()) return
        wv.loadDataWithBaseURL(null, html, "text/html", "utf-8", null)
    }

    /** `user-agent` prop setter. Must be set before any load to take effect. */
    fun setUserAgent(ua: String) {
        val wv = view ?: return
        wv.settings.userAgentString = ua.ifEmpty { null }
    }

    /** `javascript-enabled` prop setter ("true"/"false"). */
    @SuppressLint("SetJavaScriptEnabled")
    fun setJavascriptEnabled(flag: String) {
        jsEnabled = flag == "true"
        val wv = view ?: return
        wv.settings.javaScriptEnabled = jsEnabled
    }

    /**
     * `scroll-enabled` prop setter ("true"/"false").
     *
     * Disabling hides the scroll bars and installs a touch listener that
     * consumes pointer scroll/fling events (ACTION_MOVE / ACTION_UP) so
     * the user cannot scroll. Enabling removes the listener and restores
     * the scroll bars. Programmatic scrollTo() calls are unaffected.
     */
    fun setScrollEnabled(flag: String) {
        scrollEnabled = flag != "false"
        val wv = view ?: return
        wv.isVerticalScrollBarEnabled = scrollEnabled
        wv.isHorizontalScrollBarEnabled = scrollEnabled
        if (!scrollEnabled) {
            wv.setOnTouchListener { _, event ->
                // Consume touch-driven scroll / fling; allow taps (ACTION_DOWN
                // and ACTION_UP for single tap) so links still work.
                event.action == android.view.MotionEvent.ACTION_MOVE
            }
        } else {
            wv.setOnTouchListener(null)
        }
    }

    /**
     * `origin-whitelist` prop setter. Expects a JSON array string of glob
     * patterns (the default permits http and https origins). Falls back to a
     * permissive list on parse errors so a typo doesn't silently block all
     * navigation.
     *
     * Hand-parsed with org.json (bundled in AOSP) to avoid a serde dep.
     */
    fun setOriginWhitelist(json: String) {
        if (json.isBlank()) {
            originWhitelist = listOf("https://*", "http://*")
            return
        }
        try {
            val arr = org.json.JSONArray(json)
            val list = mutableListOf<String>()
            for (i in 0 until arr.length()) {
                list.add(arr.getString(i))
            }
            originWhitelist = list
        } catch (_: Throwable) {
            // Malformed JSON — keep the existing list rather than clearing.
        }
    }

    // -------------------------------------------------------------------------
    // Callable UI methods (invoked from WebViewModule's Function blocks)
    // -------------------------------------------------------------------------

    fun reload() {
        view?.reload()
    }

    fun goBack() {
        val wv = view ?: return
        if (wv.canGoBack()) wv.goBack()
    }

    fun goForward() {
        val wv = view ?: return
        if (wv.canGoForward()) wv.goForward()
    }

    fun stopLoading() {
        view?.stopLoading()
    }

    /**
     * Rust → page message delivery.
     *
     * Calls `window.whisker._receive(data)` in the page context. `data` is
     * JSON-encoded (wrapped in double-quotes) when it's a plain string so the
     * page receives a JS string rather than a bare token.
     */
    fun postMessageToPage(data: String) {
        val wv = view ?: return
        // Encode data as a JSON string literal ("…") so it arrives in the
        // page as a JS string. Characters that would break the JS string are
        // escaped: \ → \\, " → \", newlines → \n, carriage returns → \r.
        val encoded = buildString {
            append('"')
            for (c in data) {
                when (c) {
                    '\\' -> append("\\\\")
                    '"' -> append("\\\"")
                    '\n' -> append("\\n")
                    '\r' -> append("\\r")
                    else -> append(c)
                }
            }
            append('"')
        }
        wv.evaluateJavascript("window.whisker._receive($encoded)", null)
    }

    /**
     * Run [script] in the page and return the result as a
     * `WhiskerValue.Map(mapOf("value" to …))`.
     *
     * Android's `evaluateJavascript` delivers the script's return value
     * as a **JSON-encoded string**:
     *   - JS `"hello"`    → callback receives `"\"hello\""` (a string that
     *                        starts with a quote character).
     *   - JS `42`         → callback receives `"42"`.
     *   - JS `null/undefined` → callback receives `"null"`.
     *
     * We strip one layer of JSON encoding (outer double-quotes + escape
     * sequences) so the Rust side gets the bare string ("hello", not
     * "\"hello\""). Non-string results (numbers, booleans, null) are
     * returned verbatim as the token string ("42", "null").
     *
     * The result is delivered asynchronously by the JS engine; we
     * capture it in the ValueCallback and return it as the Function's
     * synchronous WhiskerValue. For fire-and-forget callers the result
     * is discarded by the bridge; for invoke_typed callers it is
     * delivered via the async result channel.
     *
     * NOTE: per repo memory, Android result-returning element methods
     * require the invoke_async bridge path wired through
     * lynx_native_renderer.cc, which is iOS-only-compiled in Lynx
     * 3.8.0-whisker.1. Implement correctly here so the method is
     * available once the fork wires result-method plumbing on Android.
     */
    fun evaluateJs(script: String): WhiskerValue {
        val wv = view ?: return WhiskerValue.Map(mapOf("value" to WhiskerValue.Str("")))
        var result: String = ""
        wv.evaluateJavascript(script) { jsonEncodedResult ->
            // jsonEncodedResult is null when the WebView has been destroyed.
            val raw = jsonEncodedResult ?: "null"
            result = jsonDecodeString(raw)
        }
        // The ValueCallback is delivered on the UI thread (same looper),
        // so by the time evaluateJavascript returns the callback has run
        // (within the same message dispatch) when the calling code is also
        // on the main thread. This matches whisker-input's getValue pattern.
        return WhiskerValue.Map(mapOf("value" to WhiskerValue.Str(result)))
    }

    /** Synchronous bool check — can the WebView navigate back? */
    fun queryCanGoBack(): WhiskerValue =
        WhiskerValue.Bool(view?.canGoBack() ?: false)

    /** Synchronous bool check — can the WebView navigate forward? */
    fun queryCanGoForward(): WhiskerValue =
        WhiskerValue.Bool(view?.canGoForward() ?: false)

    // -------------------------------------------------------------------------
    // Inner JS bridge interface
    // -------------------------------------------------------------------------

    /**
     * Receives `window.whisker.postMessage(data)` calls from the page.
     *
     * Methods annotated with `@JavascriptInterface` run on the JavaBridge
     * background thread. We hop back to the UI thread via `view?.post { … }`
     * before touching any View API or Lynx emitter state (required both for
     * thread safety and to avoid the RefCell-reentry panic during teardown).
     */
    private inner class WhiskerBridge {
        @JavascriptInterface
        fun postMessage(data: String) {
            view?.post {
                WhiskerCustomEvent.dispatch(
                    ui = this@WhiskerWebView,
                    name = "message",
                    params = mapOf("data" to data),
                )
            }
        }
    }

    // -------------------------------------------------------------------------
    // Internal helpers
    // -------------------------------------------------------------------------

    /**
     * Strip one layer of JSON string encoding from a value delivered by
     * `evaluateJavascript`'s ValueCallback.
     *
     * Android's WebView JSON-encodes the JS return value:
     *   - A JS string `"hello"` arrives as the Java String `"\"hello\""`.
     *   - A number, boolean, or null arrives as the bare token: `"42"`.
     *
     * This function returns the bare string content for JSON string tokens
     * and the verbatim token for everything else.
     *
     * Hand-rolled to avoid pulling in a JSON library dep for this single
     * operation. Handles the common escape sequences: \\, \", \n, \r, \t,
     * \uXXXX. Unexpected or malformed input is returned unchanged.
     */
    private fun jsonDecodeString(s: String): String {
        if (s.length < 2 || !s.startsWith('"') || !s.endsWith('"')) {
            // Not a JSON string literal — return the token verbatim.
            return s
        }
        // Peel the outer quotes and unescape.
        val inner = s.substring(1, s.length - 1)
        val out = StringBuilder(inner.length)
        var i = 0
        while (i < inner.length) {
            val c = inner[i]
            if (c == '\\' && i + 1 < inner.length) {
                when (val esc = inner[i + 1]) {
                    '"', '\\', '/' -> { out.append(esc); i += 2 }
                    'n' -> { out.append('\n'); i += 2 }
                    'r' -> { out.append('\r'); i += 2 }
                    't' -> { out.append('\t'); i += 2 }
                    'b' -> { out.append('\b'); i += 2 }
                    'u' -> {
                        // \uXXXX — 4 hex digits.
                        if (i + 5 < inner.length) {
                            val hex = inner.substring(i + 2, i + 6)
                            val code = hex.toIntOrNull(16)
                            if (code != null) {
                                out.append(code.toChar())
                                i += 6
                            } else {
                                out.append(c); i++
                            }
                        } else {
                            out.append(c); i++
                        }
                    }
                    else -> { out.append(c); i++ }
                }
            } else {
                out.append(c)
                i++
            }
        }
        return out.toString()
    }

    /**
     * Match a URL against a glob pattern. Only `*` (wildcard) is meaningful;
     * all other characters are treated as literals.
     *
     * Examples: pattern "https" + slash-slash-star matches
     * "https://example.com/path" (true); a pattern ending in "example.com/"
     * + star does not match "https://other.com/" (false).
     *
     * The matching is implemented as a simple recursive descent so no regex
     * dependency is needed. Patterns from the Rust side are already validated
     * (they come from the `origin_whitelist` prop). An empty pattern list
     * is treated as "allow all" by the caller.
     */
    private fun matchesGlob(pattern: String, url: String): Boolean {
        // Convert the glob to a regex-free recursive match: split on *, check
        // each segment appears in order in the URL.
        val parts = pattern.split("*")
        if (parts.size == 1) return url == pattern // no wildcard: exact match
        var pos = 0
        for ((idx, part) in parts.withIndex()) {
            if (part.isEmpty()) continue
            val found = url.indexOf(part, pos)
            if (found == -1) return false
            // The first segment must start at pos 0 (no leading wildcard).
            if (idx == 0 && found != 0) return false
            pos = found + part.length
        }
        // The last segment must end at the end of the URL (no trailing wildcard).
        val lastPart = parts.last()
        if (lastPart.isNotEmpty() && !url.endsWith(lastPart)) return false
        return true
    }
}
