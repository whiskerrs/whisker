// Lynx UI subclass hosting a WKWebView behind a unified Whisker interface.
// Registration is driven by `WebViewModule`'s `definition()` — no
// annotations required here.
//
// `@objc(WhiskerWebViewView)` pins the Obj-C class name so the codegen
// plugin's `NSClassFromString` lookup can find it regardless of whether the
// SwiftPM-target prefix (`whisker_webview.WhiskerWebViewView`) or the bare
// form is used.
//
// ## WKWebView memory-management
//
// WKWebView retains its `WKUserContentController`, which in turn retains
// every registered `WKScriptMessageHandler`. The view object returned by
// `createView()` is owned by Lynx; if `WhiskerWebViewView` were registered
// directly as the script-message handler, a retain cycle would prevent
// deallocation. We break the cycle with a weak-proxy trampoline
// (`WeakScriptMessageProxy`) that holds `self` weakly and is the actual
// object registered with the controller. When the `WhiskerWebViewView` is
// deallocated, the proxy's `self` reference becomes `nil` and the bridge
// callback silently drops incoming messages — no crash, no cycle.
//
// ## KVO for progress
//
// `WKWebView.estimatedProgress` is observed via KVO. The observation token
// (`progressObservation`) is stored as an optional `NSKeyValueObservation`;
// holding the token strong keeps the observation alive, and setting it to
// `nil` (or letting it deinit) cancels the observation. The token is
// removed from `reset()` (Lynx teardown hook) to cancel before
// the WKWebView itself is released.
//
// ## Event dispatch
//
// Every `WhiskerCustomEvent.dispatch(...)` fires SYNCHRONOUSLY. These
// callbacks (navigation-delegate / KVO / script-message) can fire during
// Lynx teardown while a renderer op is on the Rust stack; that used to
// re-enter `dispatch_event` → a second renderer borrow → "RefCell already
// borrowed" abort, so dispatch was deferred a runloop tick — the one-tick
// delay of whisker #3. The Rust renderer is now re-entrancy-safe (shared
// `with_renderer` borrow + `&self` `DynRenderer` methods + FFI-scoped
// per-field `RefCell`s in `BridgeRenderer`), so synchronous re-entrant
// dispatch is safe and the deferral was removed. See the emission helpers
// below.
//
// ## Event payload shape
//
// Params are passed DIRECTLY (e.g. `["url": urlString]`). Do NOT wrap in a
// `detail` key — the iOS bridge's `LynxCustomEvent.params` normalisation
// already places the dispatched params under `detail` in the event body, so
// the Rust structs (`NavEvent { detail: { url } }`, etc.) read the correct
// shape. Double-wrapping would produce `detail: { detail: { url } }` and
// every handler would receive the default-deserialized empty value.
//
// ## Origin-whitelist glob matching
//
// Pattern `*` matches any string; `?` is NOT a wildcard here (matching the
// Rust contract's note about `*`-only wildcards). The match is performed
// against the full URL string, so a pattern like `https://*` matches any
// URL whose string representation starts with `https://`. We use a simple
// shell-glob approach: split on `*`, check that the URL string contains all
// segments in order (first anchored to the start, last to the end).
//
// ## iOS 14 minimum
//
// `WKWebpagePreferences.allowsContentJavaScript` is iOS 14+. The
// Package.swift declares `.iOS(.v14)` accordingly.

import Foundation
import UIKit
import WebKit
import WhiskerModule

// MARK: - Weak proxy (retain-cycle breaker)

/// Forwarding proxy that holds `WhiskerWebViewView` weakly and is
/// registered as the `WKScriptMessageHandler`. When the owning view is
/// deallocated the proxy's weak reference becomes nil and incoming messages
/// are silently dropped.
private final class WeakScriptMessageProxy: NSObject, WKScriptMessageHandler {
    weak var target: WhiskerWebViewView?
    init(target: WhiskerWebViewView) { self.target = target }

    func userContentController(
        _ userContentController: WKUserContentController,
        didReceive message: WKScriptMessage
    ) {
        target?.handleScriptMessage(message)
    }
}

// MARK: - WhiskerWebViewView

@objc(WhiskerWebViewView)
public final class WhiskerWebViewView: WhiskerUI<UIView> {

    // MARK: - Hosted views

    /// Transparent container that fills the LynxUI frame; holds the
    /// `WKWebView` as a subview pinned to its bounds.
    private lazy var containerView: UIView = {
        let v = UIView()
        v.backgroundColor = .clear
        return v
    }()

    /// The live WKWebView. Created once in `createView()`; never replaced.
    private var webView: WKWebView!

    // MARK: - Cached prop state

    /// The last URL that was successfully requested via `setUrl`. Used to
    /// detect changes and avoid redundant reloads.
    private var lastLoadedUrl: String = ""

    /// Cached HTML string. Applied when `url` is empty.
    private var cachedHtml: String = ""

    /// Cached `url` prop. When non-empty, takes precedence over `html`.
    private var cachedUrl: String = ""

    /// Cached origin-whitelist glob patterns. Default matches any http/https.
    private var originWhitelist: [String] = ["https://*", "http://*"]

    // MARK: - KVO

    /// Retaining this token keeps the `estimatedProgress` KVO observation
    /// alive. Setting it to `nil` cancels the observation.
    private var progressObservation: NSKeyValueObservation?

    // MARK: - LynxUI lifecycle

    @objc public override func createView() -> UIView {
        // Build the WKWebView with a configuration that includes the
        // JS bridge user script and registers the weak-proxy message handler.
        let config = WKWebViewConfiguration()

        // JS bridge: inject `window.whisker` at document start so page
        // scripts can call `window.whisker.postMessage(data)` immediately.
        let bridgeScript = WKUserScript(
            source: """
            window.whisker = window.whisker || {};
            window.whisker.postMessage = function(data) {
                var s = (typeof data === 'string') ? data : JSON.stringify(data);
                window.webkit.messageHandlers.whisker.postMessage(s);
            };
            window.whisker._receive = function(data) {
                if (window.whisker.onMessage) { window.whisker.onMessage(data); }
            };
            """,
            injectionTime: .atDocumentStart,
            forMainFrameOnly: false
        )
        config.userContentController.addUserScript(bridgeScript)

        // Register the weak proxy as the message handler to break the
        // WKUserContentController → handler retain cycle.
        let proxy = WeakScriptMessageProxy(target: self)
        config.userContentController.add(proxy, name: "whisker")

        // JavaScript is enabled by default; the `javascript-enabled` prop
        // overrides via `defaultWebpagePreferences` after construction.
        if #available(iOS 14.0, *) {
            config.defaultWebpagePreferences.allowsContentJavaScript = true
        }

        let wv = WKWebView(frame: .zero, configuration: config)
        wv.navigationDelegate = self
        self.webView = wv

        // Observe load progress via KVO. The block fires on the main queue.
        progressObservation = wv.observe(
            \.estimatedProgress,
            options: [.new]
        ) { [weak self] _, change in
            guard let self, let progress = change.newValue else { return }
            self.emitProgress(progress)
        }

        containerView.addSubview(wv)

        // Apply any props that arrived before the WKWebView existed.
        // Property setters (`setUrl`, `setHtml`) use optional chaining on
        // `webView` and silently no-op when the Lynx element is constructed
        // before `createView()` runs.  Replay them now.
        if !cachedUrl.isEmpty {
            lastLoadedUrl = cachedUrl
            if let url = URL(string: cachedUrl) {
                wv.load(URLRequest(url: url))
            }
        } else if !cachedHtml.isEmpty {
            wv.loadHTMLString(cachedHtml, baseURL: nil)
        }

        return containerView
    }

    @objc public override func frameDidChange() {
        super.frameDidChange()
        // Keep the WKWebView filling the element bounds.
        webView?.frame = self.view().bounds
    }

    /// `WhiskerUI`/`LynxUI` exposes no teardown override (`reset()` isn't
    /// part of the surface), so we clean up in `deinit`. The
    /// `WeakScriptMessageProxy` keeps the `WKUserContentController` from
    /// retaining us, and `WKWebView.navigationDelegate` is a weak
    /// property — so there's no retain cycle and `deinit` fires when Lynx
    /// releases the view (on the main thread). Stop loading, drop the
    /// handler/scripts, and cancel KVO to free the web process promptly.
    deinit {
        progressObservation = nil
        webView?.stopLoading()
        webView?.navigationDelegate = nil
        webView?.configuration.userContentController.removeScriptMessageHandler(forName: "whisker")
        webView?.configuration.userContentController.removeAllUserScripts()
    }

    // MARK: - Public setters (called by WebViewModule's Prop closures)

    // ---- Content ---------------------------------------------------------

    /// Set the `url` prop. When non-empty, loads that URL in the web view.
    /// A change-detection guard prevents redundant reloads when the same
    /// URL is re-applied (e.g. reactive re-renders that don't change the
    /// value).
    public func setUrl(_ urlString: String) {
        cachedUrl = urlString
        guard !urlString.isEmpty else { return }
        // Only reload when the URL has actually changed.
        guard urlString != lastLoadedUrl else { return }
        lastLoadedUrl = urlString
        guard let url = URL(string: urlString) else { return }
        webView?.load(URLRequest(url: url))
    }

    /// Set the `html` prop. Applied only when `url` is empty so `url` takes
    /// precedence.
    public func setHtml(_ html: String) {
        cachedHtml = html
        guard cachedUrl.isEmpty else { return }
        webView?.loadHTMLString(html, baseURL: nil)
    }

    // ---- Browser behaviour -----------------------------------------------

    public func setUserAgent(_ ua: String) {
        webView?.customUserAgent = ua.isEmpty ? nil : ua
    }

    public func setJavascriptEnabled(_ s: String) {
        guard #available(iOS 14.0, *) else { return }
        webView?.configuration.defaultWebpagePreferences.allowsContentJavaScript = (s != "false")
    }

    public func setScrollEnabled(_ s: String) {
        webView?.scrollView.isScrollEnabled = (s != "false")
    }

    /// Parses a JSON-array string like `["https://*","http://*"]` into the
    /// local `originWhitelist` used by `decidePolicyFor`.
    public func setOriginWhitelist(_ json: String) {
        guard !json.isEmpty else { return }
        // Hand-roll the parse: the only legal values are JSON string arrays,
        // produced by the Rust `origin_whitelist_json` helper. We walk the
        // string extracting quoted tokens rather than pulling in Foundation's
        // JSONSerialization to keep the dependency surface minimal.
        var patterns: [String] = []
        var idx = json.startIndex
        while idx < json.endIndex {
            // Scan to the opening quote of a string token.
            guard let open = json[idx...].firstIndex(of: "\"") else { break }
            let afterOpen = json.index(after: open)
            guard afterOpen < json.endIndex else { break }
            // Scan to the closing quote, handling `\"` escapes.
            var end = afterOpen
            while end < json.endIndex {
                if json[end] == "\\" {
                    let next = json.index(after: end)
                    if next < json.endIndex { end = json.index(after: next) } else { end = next }
                } else if json[end] == "\"" {
                    break
                } else {
                    end = json.index(after: end)
                }
            }
            // Decode the token, unescaping `\"` → `"` and `\\` → `\`.
            let raw = String(json[afterOpen..<end])
            let unescaped = raw
                .replacingOccurrences(of: "\\\"", with: "\"")
                .replacingOccurrences(of: "\\\\", with: "\\")
            patterns.append(unescaped)
            idx = end < json.endIndex ? json.index(after: end) : json.endIndex
        }
        if !patterns.isEmpty {
            originWhitelist = patterns
        }
    }

    // MARK: - Imperative method targets (called by WebViewModule's Function closures)

    public func reloadPage() {
        webView?.reload()
    }

    public func goBackPage() {
        webView?.goBack()
    }

    public func goForwardPage() {
        webView?.goForward()
    }

    public func stopLoadingPage() {
        webView?.stopLoading()
    }

    /// Deliver a Rust-originated string to the page's `window.whisker.onMessage`
    /// handler by evaluating `window.whisker._receive(...)` in the page context.
    public func postMessageToPage(_ data: String) {
        // JSON-encode the data string into a safe JS string literal so
        // any embedded quotes / backslashes / newlines don't break the script.
        let jsString = jsonStringLiteral(data)
        webView?.evaluateJavaScript("window.whisker._receive(\(jsString))", completionHandler: nil)
    }

    /// Evaluate arbitrary JavaScript. Returns the result as
    /// `.map(["value": .string(<stringified result>)])` to satisfy both the
    /// fire-and-forget (`invoke`) and async-result (`invoke_typed`) callers.
    ///
    /// iOS `evaluateJavaScript(_:completionHandler:)` is async internally,
    /// but the Lynx `Function` dispatch is synchronous. We handle this by
    /// running the evaluation on a synchronous semaphore within the same
    /// main-thread call, which is safe here because `evaluateJavaScript`
    /// dispatches its completion on the main queue itself — the pattern
    /// works only because WKWebView uses an internal background JS-thread
    /// and posts completion back to main. We use a short timeout (3 s)
    /// to avoid deadlocking when the page hangs.
    ///
    /// If the semaphore-wait approach is not acceptable in future (e.g.
    /// strict no-wait policy on main), the alternative is to have Lynx
    /// dispatch `evaluateJavaScript` through an async channel. For now,
    /// this matches the pattern used by other Whisker modules that need
    /// sync-result semantics from async platform APIs.
    public func evaluateJavaScript(_ script: String) -> WhiskerValue {
        guard let wv = webView else {
            return .map(["value": .string("")])
        }
        // WKWebView's `evaluateJavaScript` completion block fires on the
        // main queue. We must NOT block the main thread with a semaphore
        // from the main queue — that would deadlock.
        //
        // Instead, we return `.map(["value": .string("")])` as a sentinel and
        // fire the evaluation for side-effects only. For the async-result path
        // (`invoke_typed`) this means the result is always the empty string;
        // fire-and-forget (`invoke`) ignores the return value anyway.
        //
        // NOTE: A true async result requires an `AsyncFunction` DSL entry
        // (not yet shipped in L-2a). When `AsyncFunction` lands, replace this
        // with a Promise / continuation. Until then callers that need the JS
        // return value should use the Lynx result-method mechanism directly.
        wv.evaluateJavaScript(script, completionHandler: nil)
        return .map(["value": .string("")])
    }

    public func canGoBackResult() -> WhiskerValue {
        return .bool(webView?.canGoBack ?? false)
    }

    public func canGoForwardResult() -> WhiskerValue {
        return .bool(webView?.canGoForward ?? false)
    }

    // MARK: - Script-message handler (called by the proxy)

    /// Called by `WeakScriptMessageProxy` when the page invokes
    /// `window.whisker.postMessage(...)`.
    func handleScriptMessage(_ message: WKScriptMessage) {
        let data = message.body as? String ?? ""
        emitMessage(data)
    }

    // MARK: - Event emission helpers

    // These dispatch SYNCHRONOUSLY. Navigation-delegate / KVO /
    // script-message callbacks can fire during Lynx teardown while a
    // renderer op (`remove_child`) is on the Rust stack. Previously that
    // re-entered `dispatch_event` → a second `with_renderer` borrow →
    // "RefCell already borrowed" abort, so we deferred one main-runloop
    // tick (`DispatchQueue.main.async`) to dodge it — the one-tick-late
    // delivery of whisker #3.
    //
    // The Rust renderer is now re-entrancy-safe: `DynRenderer` methods
    // take `&self`, `BridgeRenderer` keeps its state behind per-field
    // `RefCell`s with FFI-scoped borrows, and `with_renderer` takes a
    // SHARED borrow, so a synchronous re-entrant dispatch during teardown
    // is granted rather than aborting. See
    // `crates/whisker-runtime/src/view/renderer.rs` and
    // `crates/whisker-driver/src/lynx/renderer.rs`. The deferral is no
    // longer needed; removing it collapses the one-tick delay so webview
    // events (`load`, `navigation`, `message`, …) deliver on the same tick.

    private func emitLoadStart(_ urlString: String) {
        WhiskerCustomEvent.dispatch(from: self, name: "load_start", params: ["url": urlString])
    }

    private func emitLoad(_ urlString: String) {
        WhiskerCustomEvent.dispatch(from: self, name: "load", params: ["url": urlString])
    }

    private func emitError(urlString: String, code: Int, description: String) {
        WhiskerCustomEvent.dispatch(from: self, name: "error", params: [
            "url": urlString,
            "code": code,
            "description": description,
        ])
    }

    private func emitNavigation(_ urlString: String) {
        WhiskerCustomEvent.dispatch(from: self, name: "navigation", params: ["url": urlString])
    }

    private func emitProgress(_ progress: Double) {
        WhiskerCustomEvent.dispatch(from: self, name: "progress", params: ["progress": progress])
    }

    private func emitMessage(_ data: String) {
        WhiskerCustomEvent.dispatch(from: self, name: "message", params: ["data": data])
    }

    // MARK: - Origin-whitelist matching

    /// Returns `true` if `urlString` is allowed by at least one pattern in
    /// `originWhitelist`. Each pattern uses `*` as a wildcard matching any
    /// substring; no other wildcards. Matching is performed against the full
    /// URL string.
    private func isAllowed(_ urlString: String) -> Bool {
        for pattern in originWhitelist {
            if globMatch(pattern: pattern, string: urlString) { return true }
        }
        return false
    }

    /// Shell-style glob match: `*` matches any substring (including empty).
    /// No `?` wildcard — the Rust contract documents `*` only.
    private func globMatch(pattern: String, string: String) -> Bool {
        // Split the pattern on `*`. All segments must appear in order in
        // `string`; the first segment is anchored to the start, the last to
        // the end.
        let parts = pattern.components(separatedBy: "*")
        guard !parts.isEmpty else { return true }

        var remaining = string[...]

        for (i, part) in parts.enumerated() {
            if part.isEmpty { continue }
            if i == 0 {
                // First segment must be a prefix.
                guard remaining.hasPrefix(part) else { return false }
                remaining = remaining.dropFirst(part.count)
            } else if i == parts.count - 1 {
                // Last segment must be a suffix of what remains.
                guard remaining.hasSuffix(part) else { return false }
            } else {
                // Middle segment must appear somewhere in `remaining`.
                guard let range = remaining.range(of: part) else { return false }
                remaining = remaining[range.upperBound...]
            }
        }
        return true
    }

    // MARK: - JS string helpers

    /// JSON-encodes a Swift `String` into a JavaScript string literal
    /// (including surrounding double quotes) safe to embed directly in
    /// a `<script>` call. Escapes `"`, `\`, and control characters.
    private func jsonStringLiteral(_ s: String) -> String {
        var out = "\""
        for ch in s.unicodeScalars {
            switch ch {
            case "\"": out += "\\\""
            case "\\": out += "\\\\"
            case "\n": out += "\\n"
            case "\r": out += "\\r"
            case "\t": out += "\\t"
            default:
                if ch.value < 0x20 {
                    out += String(format: "\\u%04X", ch.value)
                } else {
                    out += String(ch)
                }
            }
        }
        out += "\""
        return out
    }
}

// MARK: - WKNavigationDelegate

extension WhiskerWebViewView: WKNavigationDelegate {

    public func webView(
        _ webView: WKWebView,
        decidePolicyFor navigationAction: WKNavigationAction,
        decisionHandler: @escaping (WKNavigationActionPolicy) -> Void
    ) {
        // Only apply the whitelist to main-frame navigations; sub-resource
        // loads (images, XHR, etc.) are always allowed.
        guard navigationAction.targetFrame?.isMainFrame == true else {
            decisionHandler(.allow)
            return
        }

        let urlString = navigationAction.request.url?.absoluteString ?? ""
        let scheme = navigationAction.request.url?.scheme?.lowercased() ?? ""

        // Real web navigations (`http` / `https`) are gated by the origin
        // whitelist.
        if scheme == "http" || scheme == "https" {
            if !isAllowed(urlString) {
                decisionHandler(.cancel)
                return
            }
            // Emit `navigation` only for real web navigations.
            emitNavigation(urlString)
            decisionHandler(.allow)
            return
        }

        // Inline / generated content. `loadHTMLString(_:baseURL:)`
        // navigates to `about:blank`; `data:` / `blob:` back inline
        // documents and generated resources. These never match a
        // `https://*` / `http://*` pattern, so the whitelist would wrongly
        // cancel them and render a blank page — allow them explicitly.
        if scheme == "about" || scheme == "data" || scheme == "blob" {
            decisionHandler(.allow)
            return
        }

        // Everything else — notably `file:` (local file disclosure risk),
        // plus `javascript:` and custom deep-link schemes — is denied.
        // The component exposes no file-access prop, so there is no
        // legitimate in-webview navigation to those schemes; fail closed.
        decisionHandler(.cancel)
    }

    public func webView(
        _ webView: WKWebView,
        didStartProvisionalNavigation navigation: WKNavigation!
    ) {
        let urlString = webView.url?.absoluteString ?? ""
        emitLoadStart(urlString)
    }

    public func webView(
        _ webView: WKWebView,
        didFinish navigation: WKNavigation!
    ) {
        let urlString = webView.url?.absoluteString ?? ""
        emitLoad(urlString)
    }

    public func webView(
        _ webView: WKWebView,
        didFail navigation: WKNavigation!,
        withError error: Error
    ) {
        let urlString = webView.url?.absoluteString ?? ""
        let nsErr = error as NSError
        emitError(urlString: urlString, code: nsErr.code, description: nsErr.localizedDescription)
    }

    public func webView(
        _ webView: WKWebView,
        didFailProvisionalNavigation navigation: WKNavigation!,
        withError error: Error
    ) {
        // `didFailProvisionalNavigation` fires before a committed URL is
        // available, so fall back to the request URL stored on the webView.
        let urlString = webView.url?.absoluteString
            ?? webView.backForwardList.currentItem?.url.absoluteString
            ?? ""
        let nsErr = error as NSError
        emitError(urlString: urlString, code: nsErr.code, description: nsErr.localizedDescription)
    }
}
