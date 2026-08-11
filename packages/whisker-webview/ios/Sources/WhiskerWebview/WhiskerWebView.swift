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
// WKWebView retains its `WKUserContentController`, which retains every
// registered `WKScriptMessageHandler`. Registering `WhiskerWebViewView`
// directly would therefore form a retain cycle and the Lynx-owned view
// would never deallocate. `WeakScriptMessageProxy` breaks it: it holds
// `self` weakly, so once the view is gone the bridge callback silently
// drops incoming messages.
//
// ## Event dispatch
//
// Every `WhiskerCustomEvent.dispatch(...)` fires SYNCHRONOUSLY.
// Navigation-delegate / KVO / script-message callbacks can fire during
// Lynx teardown while a renderer op is on the Rust stack, which is only
// safe because the Rust renderer is re-entrancy-safe (whisker #3: shared
// `with_renderer` borrow, `&self` `DynRenderer` methods, FFI-scoped
// per-field `RefCell`s in `BridgeRenderer`).
//
// ## Event payload shape
//
// Params are passed DIRECTLY (e.g. `["url": urlString]`). Do NOT wrap in a
// `detail` key — the iOS bridge's `LynxCustomEvent.params` normalisation
// already places the dispatched params under `detail` in the event body, so
// the Rust structs (`NavEvent { detail: { url } }`, etc.) read the correct
// shape. Double-wrapping produces `detail: { detail: { url } }` and every
// handler receives the default-deserialized empty value.
//
// ## Origin-whitelist glob matching
//
// `*` matches any substring and is the only wildcard, matching the Rust
// contract. Matching is against the full URL string, so `https://*` admits
// any URL whose string starts with `https://`.

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

    private var lastLoadedUrl: String = ""

    /// Cached HTML string. Applied when `url` is empty.
    private var cachedHtml: String = ""

    /// Cached `url` prop. When non-empty, takes precedence over `html`.
    private var cachedUrl: String = ""

    private var originWhitelist: [String] = ["https://*", "http://*"]

    // MARK: - KVO

    /// Retaining this token is what keeps the `estimatedProgress`
    /// observation alive; clearing it cancels the observation.
    private var progressObservation: NSKeyValueObservation?

    // MARK: - LynxUI lifecycle

    @objc public override func createView() -> UIView {
        let config = WKWebViewConfiguration()

        // Document-start injection so page scripts can call
        // `window.whisker.postMessage(data)` immediately.
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

        // The proxy, not `self` — see the retain-cycle note at the top.
        let proxy = WeakScriptMessageProxy(target: self)
        config.userContentController.add(proxy, name: "whisker")

        if #available(iOS 14.0, *) {
            config.defaultWebpagePreferences.allowsContentJavaScript = true
        }

        let wv = WKWebView(frame: .zero, configuration: config)
        wv.navigationDelegate = self
        self.webView = wv

        // The block fires on the main queue.
        progressObservation = wv.observe(
            \.estimatedProgress,
            options: [.new]
        ) { [weak self] _, change in
            guard let self, let progress = change.newValue else { return }
            self.emitProgress(progress)
        }

        containerView.addSubview(wv)

        // `setUrl` / `setHtml` optional-chain on `webView` and silently
        // no-op when props arrive before `createView()` runs, so replay
        // whatever was cached.
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
        webView?.frame = self.view().bounds
    }

    /// `WhiskerUI` / `LynxUI` exposes no teardown override, so cleanup has
    /// to happen in `deinit`. That works because nothing retains this
    /// object back: `WeakScriptMessageProxy` stands in for us on the
    /// `WKUserContentController` and `navigationDelegate` is weak, so
    /// `deinit` does fire — on the main thread — when Lynx releases the
    /// view, and the web process is freed promptly.
    deinit {
        progressObservation = nil
        webView?.stopLoading()
        webView?.navigationDelegate = nil
        webView?.configuration.userContentController.removeScriptMessageHandler(forName: "whisker")
        webView?.configuration.userContentController.removeAllUserScripts()
    }

    // MARK: - Public setters (called by WebViewModule's Prop closures)

    // ---- Content ---------------------------------------------------------

    /// Set the `url` prop. The change-detection guard stops reactive
    /// re-renders that don't touch the value from reloading the page.
    public func setUrl(_ urlString: String) {
        cachedUrl = urlString
        guard !urlString.isEmpty else { return }
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
        // Hand-rolled quoted-token scan rather than JSONSerialization: the
        // only legal input is a JSON string array from the Rust
        // `origin_whitelist_json` helper.
        var patterns: [String] = []
        var idx = json.startIndex
        while idx < json.endIndex {
            guard let open = json[idx...].firstIndex(of: "\"") else { break }
            let afterOpen = json.index(after: open)
            guard afterOpen < json.endIndex else { break }
            // Scan to the closing quote, honouring `\"` escapes.
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
        // Encoded as a JS string literal so embedded quotes, backslashes,
        // and newlines can't break the injected script.
        let jsString = jsonStringLiteral(data)
        webView?.evaluateJavaScript("window.whisker._receive(\(jsString))", completionHandler: nil)
    }

    /// Evaluate arbitrary JavaScript for its side effects. The returned
    /// `value` is always the empty string — the completion fires after
    /// this synchronous dispatch returns; use
    /// [`evaluateJavaScript(_:resolving:)`] for the result.
    public func evaluateJavaScript(_ script: String) -> WhiskerValue {
        guard let wv = webView else {
            return .map(["value": .string("")])
        }
        wv.evaluateJavaScript(script, completionHandler: nil)
        return .map(["value": .string("")])
    }

    /// Evaluate JavaScript and settle `promise` from the completion:
    /// the result as a JSON-encoded string (`"null"` when the script
    /// yields no value), matching Android's `evaluateJavascript`
    /// callback convention; a JS exception rejects.
    public func evaluateJavaScript(_ script: String, resolving promise: WhiskerPromise) {
        guard let wv = webView else {
            promise.resolve(.string("null"))
            return
        }
        wv.evaluateJavaScript(script) { value, error in
            if let error = error {
                // The JS exception text rides in userInfo, not
                // localizedDescription.
                let ns = error as NSError
                let detail = ns.userInfo["WKJavaScriptExceptionMessage"] as? String
                    ?? error.localizedDescription
                promise.reject("evaluateJavaScript failed: \(detail)")
                return
            }
            guard let value = value else {
                promise.resolve(.string("null"))
                return
            }
            // The precheck matters: `JSONSerialization.data` raises an
            // (uncatchable) NSException for non-JSON top-level objects.
            guard JSONSerialization.isValidJSONObject(value)
                || value is NSString || value is NSNumber || value is NSNull,
                let data = try? JSONSerialization.data(
                    withJSONObject: value, options: .fragmentsAllowed),
                let json = String(data: data, encoding: .utf8)
            else {
                promise.reject("evaluateJavaScript: result is not JSON-encodable")
                return
            }
            promise.resolve(.string(json))
        }
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

    // These dispatch SYNCHRONOUSLY, which is only safe because the Rust
    // renderer is re-entrancy-safe: `DynRenderer` methods take `&self`,
    // `BridgeRenderer` keeps its state behind per-field `RefCell`s with
    // FFI-scoped borrows, and `with_renderer` takes a SHARED borrow
    // (whisker #3). Navigation-delegate / KVO / script-message callbacks can
    // fire during Lynx teardown while `remove_child` is on the Rust stack,
    // so a re-entrant dispatch is granted rather than aborting. Deferring a
    // runloop tick instead would cost every webview event a tick of latency.

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
    /// `originWhitelist`.
    private func isAllowed(_ urlString: String) -> Bool {
        for pattern in originWhitelist {
            if globMatch(pattern: pattern, string: urlString) { return true }
        }
        return false
    }

    /// Shell-style glob match: `*` matches any substring (including empty).
    /// No `?` wildcard — the Rust contract documents `*` only.
    private func globMatch(pattern: String, string: String) -> Bool {
        let parts = pattern.components(separatedBy: "*")
        guard !parts.isEmpty else { return true }

        var remaining = string[...]

        for (i, part) in parts.enumerated() {
            if part.isEmpty { continue }
            if i == 0 {
                guard remaining.hasPrefix(part) else { return false }
                remaining = remaining.dropFirst(part.count)
            } else if i == parts.count - 1 {
                guard remaining.hasSuffix(part) else { return false }
            } else {
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
        // Sub-resource loads (images, XHR) are always allowed; only
        // main-frame navigation is gated.
        guard navigationAction.targetFrame?.isMainFrame == true else {
            decisionHandler(.allow)
            return
        }

        let urlString = navigationAction.request.url?.absoluteString ?? ""
        let scheme = navigationAction.request.url?.scheme?.lowercased() ?? ""

        // Only real web navigations are gated by the origin whitelist.
        if scheme == "http" || scheme == "https" {
            if !isAllowed(urlString) {
                // Denied, but still observable — see the note on the
                // "everything else" branch below.
                emitNavigation(urlString)
                decisionHandler(.cancel)
                return
            }
            emitNavigation(urlString)
            decisionHandler(.allow)
            return
        }

        // Inline / generated content must bypass the whitelist:
        // `loadHTMLString(_:baseURL:)` navigates to `about:blank`, and
        // `data:` / `blob:` back inline documents. None of them match a
        // `https://*` / `http://*` pattern, so gating them would cancel the
        // load and render a blank page.
        if scheme == "about" || scheme == "data" || scheme == "blob" {
            decisionHandler(.allow)
            return
        }

        // Everything else fails closed — notably `file:`, a local-file
        // disclosure risk, plus `javascript:` and custom deep-link schemes.
        // The component exposes no file-access prop, so no legitimate
        // in-webview navigation targets them.
        //
        // `navigation` is still emitted before cancelling: a custom scheme
        // is exactly how an in-app OAuth flow's redirect URI surfaces, and
        // since WKWebView can never load it, this denial is the only place
        // Rust can observe the attempted URL — and thus the auth code in its
        // query string — without a separate native module.
        emitNavigation(urlString)
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
