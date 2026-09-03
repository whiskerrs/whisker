// Whisker module view hosting a native android.webkit.WebView. Registration
// is driven by `WebViewModule`'s `definition()`, not by annotations on
// this class.
//
// ## Content loading
//
// `url` and `html` are mutually exclusive; a non-empty `url` wins. `url`
// is a one-way-down controlled prop: `lastLoadedUrl` tracks only what the
// prop asked for, so internal navigations (redirects, pushState) are
// never echoed back as a fresh loadUrl().
//
// ## JS bridge
//
// `window.whisker` is injected at document start via
// WebViewCompat.addDocumentStartJavaScript, falling back to onPageStarted
// injection where that feature is unsupported. The shim wires
// `window.whisker.postMessage(data)` up to the JavascriptInterface, and
// `window.whisker._receive(…)` down from `postMessageToPage`.
//
// ## Event dispatch
//
// WebViewClient / WebChromeClient callbacks fire on the UI thread and
// dispatch SYNCHRONOUSLY. They can arrive while the Host is mid-teardown, and
// that is only safe because the Rust renderer is re-entrancy-safe
// (whisker #3: `with_renderer` takes a shared borrow and every renderer
// field borrow is scoped so it never spans a re-entrant FFI call).
//
// The ONE exception is `@JavascriptInterface`, which fires on JavaBridge,
// a background thread. That path must hop to the UI thread via
// `view().post { … }` before touching any Android View / Host event emitter
// state — a genuine thread transition, not a reentrancy guard.
//
// ## Teardown
//
// An OnAttachStateChangeListener tears the web process down on detach so
// the renderer process is released promptly rather than leaking after
// the Host removes the element.

package rs.whisker.elements.webview

import android.annotation.SuppressLint
import android.content.Context
import android.os.Build
import android.os.Handler
import android.os.Looper
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

    /** The URL most recently passed to loadUrl(). Re-loading the same URL
     *  would interrupt an in-flight navigation and reset the browser's own
     *  history, so a repeat of this value is skipped. */
    private var lastLoadedUrl: String? = null

    /** The current `html` prop value. Only rendered when `url` is empty. */
    private var pendingHtml: String = ""

    private var scrollEnabled: Boolean = true

    private val cleanupHandler = Handler(Looper.getMainLooper())

    /** Glob patterns; an empty list means "allow all". */
    private var originWhitelist: List<String> = listOf("https://*", "http://*")

    // -------------------------------------------------------------------------
    // View creation
    // -------------------------------------------------------------------------

    @SuppressLint("SetJavaScriptEnabled")
    override fun createView(context: Context): android.webkit.WebView {
        val wv = android.webkit.WebView(context)

        wv.settings.apply {
            javaScriptEnabled = false
            domStorageEnabled = true
            // The component never loads file:// URLs, so denying local-file
            // and content:// access costs nothing and stops a file:// page —
            // or a redirect to one — reading app-sandbox or device files.
            // `allowFileAccess` defaults to true below API 30.
            allowFileAccess = false
            allowContentAccess = false
            // The default whitelist includes http://*, so mixed content has
            // to be permitted for those pages to render.
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP) {
                mixedContentMode = WebSettings.MIXED_CONTENT_COMPATIBILITY_MODE
            }
        }

        // Installed before any load, so it is present by the time page
        // scripts run.
        wv.addJavascriptInterface(WhiskerBridge(), "__whisker_android")

        // The onPageStarted fallback injects slightly later than a
        // document-start script, but still before user scripts that wait on
        // DOMContentLoaded.
        val shimInjectedViaCompat = if (WebViewFeature.isFeatureSupported(
                WebViewFeature.DOCUMENT_START_SCRIPT)
        ) {
            WebViewCompat.addDocumentStartJavaScript(wv, JS_SHIM, setOf("*"))
            true
        } else {
            false
        }

        // Inner objects so they can capture `shimInjectedViaCompat` without
        // an extra field.
        wv.webViewClient = object : WebViewClient() {
            override fun onPageStarted(
                view: android.webkit.WebView,
                url: String?,
                favicon: android.graphics.Bitmap?,
            ) {
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
                // Sub-resource errors (images, fonts) would spam the Rust
                // side with non-actionable events.
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

            @Suppress("OVERRIDE_DEPRECATION")
            override fun shouldOverrideUrlLoading(
                view: android.webkit.WebView,
                url: String?,
            ): Boolean {
                val safeUrl = url ?: return false
                return handleNavigation(view, safeUrl)
            }

            /**
             * Apply the origin whitelist and emit `navigation`. Returns true
             * to block (URL not whitelisted), false to let the load proceed.
             *
             * `navigation` is emitted in BOTH cases, not just when allowed.
             * A custom scheme — an in-app OAuth flow's redirect URI, say —
             * never matches the whitelist and WebView could never load it
             * anyway, so this denial is the only place Rust can observe the
             * attempted URL, and thus the auth code in its query string,
             * without a separate native module.
             */
            private fun handleNavigation(
                view: android.webkit.WebView,
                url: String,
            ): Boolean {
                val allowed = originWhitelist.isEmpty() ||
                    originWhitelist.any { pattern -> matchesGlob(pattern, url) }
                WhiskerCustomEvent.dispatch(
                    ui = this@WhiskerWebView,
                    name = "navigation",
                    params = mapOf("url" to url),
                )
                return !allowed // true = block, false = proceed with load
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

        wv.addOnAttachStateChangeListener(
            object : android.view.View.OnAttachStateChangeListener {
                override fun onViewAttachedToWindow(v: android.view.View) {}

                override fun onViewDetachedFromWindow(v: android.view.View) {
                    val webView = v as? android.webkit.WebView ?: return
                    // A Host reparent can detach and attach synchronously in
                    // one frame. Defer destruction so that path keeps the
                    // live browsing context.
                    cleanupHandler.post {
                        if (!webView.isAttachedToWindow) {
                            webView.stopLoading()
                            webView.removeJavascriptInterface("__whisker_android")
                            webView.destroy()
                        }
                    }
                }
            }
        )

        return wv
    }

    // -------------------------------------------------------------------------
    // Props called from WebViewModule
    // -------------------------------------------------------------------------

    /**
     * `url` prop setter. The differs-from-loaded check keeps every Rust
     * re-render that touches an unrelated prop from re-loading the page.
     */
    fun setUrl(incoming: String) {
        val wv = view()
        if (incoming.isEmpty()) {
            if (lastLoadedUrl != null) {
                lastLoadedUrl = null
                loadInlineContent(wv)
            }
            return
        }
        if (incoming == lastLoadedUrl) return
        lastLoadedUrl = incoming
        wv.loadUrl(incoming)
    }

    /**
     * `html` prop setter. When a `url` is active the HTML is stored but not
     * rendered — `url` takes priority.
     */
    fun setHtml(html: String) {
        pendingHtml = html
        val wv = view()
        if (lastLoadedUrl != null && lastLoadedUrl!!.isNotEmpty()) return
        loadInlineContent(wv)
    }

    /** `user-agent` prop setter. Must be set before any load to take effect. */
    fun setUserAgent(ua: String) {
        val wv = view()
        wv.settings.userAgentString = ua.ifEmpty { null }
    }

    @SuppressLint("SetJavaScriptEnabled")
    fun setJavascriptEnabled(enabled: Boolean) {
        val wv = view()
        wv.settings.javaScriptEnabled = enabled
    }

    /**
     * `scroll-enabled` prop setter. Disabling only blocks touch-driven
     * scrolling; programmatic `scrollTo()` still works.
     */
    fun setScrollEnabled(enabled: Boolean) {
        scrollEnabled = enabled
        val wv = view()
        wv.isVerticalScrollBarEnabled = scrollEnabled
        wv.isHorizontalScrollBarEnabled = scrollEnabled
        if (!scrollEnabled) {
            wv.setOnTouchListener { _, event ->
                // Consume only the drag, so ACTION_DOWN / ACTION_UP still
                // reach the page and links keep working.
                event.action == android.view.MotionEvent.ACTION_MOVE
            }
        } else {
            wv.setOnTouchListener(null)
        }
    }

    /**
     * `origin-whitelist` prop setter, taking a JSON array string of glob
     * patterns. A blank value restores the http/https default; malformed
     * JSON keeps the list in force, so a typo can't silently open up or
     * block all navigation.
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
            // Keep the list in force.
        }
    }

    // -------------------------------------------------------------------------
    // Callable UI methods (invoked from WebViewModule's Function blocks)
    // -------------------------------------------------------------------------

    fun reload() {
        view().reload()
    }

    fun goBack() {
        val wv = view()
        if (wv.canGoBack()) wv.goBack()
    }

    fun goForward() {
        val wv = view()
        if (wv.canGoForward()) wv.goForward()
    }

    fun stopLoading() {
        view().stopLoading()
    }

    /**
     * Rust → page message delivery, via `window.whisker._receive(data)`.
     */
    fun postMessageToPage(data: String) {
        val wv = view()
        // Encode as a JSON string literal so the page receives a JS string
        // rather than a bare token that would break the injected expression.
        val encoded = org.json.JSONObject.quote(data)
        wv.evaluateJavascript("window.whisker._receive($encoded)", null)
    }

    /** Run [script] in the page for its side effects. */
    fun evaluateJs(script: String) {
        view().evaluateJavascript(script, null)
    }

    // -------------------------------------------------------------------------
    // Inner JS bridge interface
    // -------------------------------------------------------------------------

    /**
     * Receives `window.whisker.postMessage(data)` calls from the page.
     *
     * `@JavascriptInterface` methods run on the JavaBridge background
     * thread, so the hop back to the UI thread via `view().post { … }` must
     * happen before any View API or Host event emitter state is touched.
     */
    private inner class WhiskerBridge {
        @JavascriptInterface
        fun postMessage(data: String) {
            view().post {
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

    private fun loadInlineContent(webView: android.webkit.WebView) {
        if (pendingHtml.isEmpty()) {
            webView.loadUrl("about:blank")
        } else {
            webView.loadDataWithBaseURL(null, pendingHtml, "text/html", "utf-8", null)
        }
    }

    /**
     * Match a URL against a glob pattern. Only `*` is meaningful; every
     * other character is a literal. An empty pattern list is "allow all",
     * handled by the caller.
     */
    private fun matchesGlob(pattern: String, url: String): Boolean {
        val parts = pattern.split("*")
        if (parts.size == 1) return url == pattern // no wildcard: exact match
        var pos = 0
        for ((idx, part) in parts.withIndex()) {
            if (part.isEmpty()) continue
            val found = url.indexOf(part, pos)
            if (found == -1) return false
            // A pattern with no leading `*` must match from the start.
            if (idx == 0 && found != 0) return false
            pos = found + part.length
        }
        // Likewise, no trailing `*` means it must reach the end.
        val lastPart = parts.last()
        if (lastPart.isNotEmpty() && !url.endsWith(lastPart)) return false
        return true
    }
}
