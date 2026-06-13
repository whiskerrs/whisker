// `whisker-webview` ModuleDefinition (iOS).
//
// Mirrors `whisker-input`'s `InputModule` shape for the `View(...)` +
// `Prop(...)` + `Function(...)` + `Events(...)` DSL surface. The codegen
// plugin discovers this `Module` subclass and emits a registration block in
// `WhiskerWebView+Generated.swift` that:
//
//   - Reads `definitionLazy.view!.viewClass` (== `WhiskerWebViewView`).
//   - Calls `LynxComponentRegistry.registerUI(viewClass, withName:
//     "whisker-webview:WebView")`.
//   - Calls `module.registerWithLynx()` so all `Prop(...)` setters +
//     `Function(...)` methods install via the Obj-C-runtime path.
//
// The `WhiskerWebViewView` Lynx UI subclass lives in `WhiskerWebView.swift`.
//
// ## Events
//
// Events are declared inside the `View(...)` block. Dispatch goes through
// `WhiskerCustomEvent.dispatch(from:name:params:)` called by the view's
// WKNavigationDelegate / WKScriptMessageHandler / KVO methods — see
// `WhiskerWebView.swift`. `Events(...)` here is declaration-only metadata.
//
// ## Prop delivery
//
// All props arrive as `WhiskerValue` (typically `.string`). Bool props
// ("true"/"false") and the JSON-array whitelist are pre-stringified by the
// Rust layer, so we always read `value.asString`.

import WhiskerModule

public final class WebViewModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("WebView")
            View(WhiskerWebViewView.self) {

                // ---- Content props ---------------------------------------

                Prop("url") { (view: WhiskerWebViewView, value: WhiskerValue) in
                    view.setUrl(value.asString ?? "")
                }
                Prop("html") { (view: WhiskerWebViewView, value: WhiskerValue) in
                    view.setHtml(value.asString ?? "")
                }

                // ---- Browser behaviour props -----------------------------

                Prop("user-agent") { (view: WhiskerWebViewView, value: WhiskerValue) in
                    view.setUserAgent(value.asString ?? "")
                }
                // "true" / "false" string sent by the Rust bool_attr() helper.
                Prop("javascript-enabled") { (view: WhiskerWebViewView, value: WhiskerValue) in
                    view.setJavascriptEnabled(value.asString ?? "true")
                }
                Prop("scroll-enabled") { (view: WhiskerWebViewView, value: WhiskerValue) in
                    view.setScrollEnabled(value.asString ?? "true")
                }
                // JSON array string, e.g. `["https://*","http://*"]`.
                Prop("origin-whitelist") { (view: WhiskerWebViewView, value: WhiskerValue) in
                    view.setOriginWhitelist(value.asString ?? "")
                }

                // ---- Events ---------------------------------------------
                //
                // Declaration-only: dispatch goes through
                // `WhiskerCustomEvent.dispatch(from:name:params:)` in
                // `WhiskerWebView.swift`. Listed here so the codegen /
                // docs scanner knows the full event surface.

                Events(
                    "load_start",
                    "load",
                    "error",
                    "navigation",
                    "progress",
                    "message"
                )

                // ---- Imperative methods ----------------------------------
                //
                // Void methods return `.null`. The async-result methods
                // (`evaluateJavaScript`, `canGoBack`, `canGoForward`) also
                // use the sync `Function` form — Lynx's
                // `<name>:withResult:` dispatch calls the closure and
                // passes the returned `WhiskerValue` straight to the
                // Rust-side `invoke_typed` awaiter via the Lynx callback.
                // This is the same pattern `InputModule.getValue` uses.

                Function("reload") { (view: WhiskerWebViewView, _: [WhiskerValue]) -> WhiskerValue in
                    view.reloadPage()
                    return .null
                }
                Function("goBack") { (view: WhiskerWebViewView, _: [WhiskerValue]) -> WhiskerValue in
                    view.goBackPage()
                    return .null
                }
                Function("goForward") { (view: WhiskerWebViewView, _: [WhiskerValue]) -> WhiskerValue in
                    view.goForwardPage()
                    return .null
                }
                Function("stopLoading") { (view: WhiskerWebViewView, _: [WhiskerValue]) -> WhiskerValue in
                    view.stopLoadingPage()
                    return .null
                }

                // Rust → JS message delivery. Args: `["<string>"]` (positional).
                Function("postMessage") { (view: WhiskerWebViewView, args: [WhiskerValue]) -> WhiskerValue in
                    if let data = args.first?.asString {
                        view.postMessageToPage(data)
                    }
                    return .null
                }

                // Evaluate arbitrary JavaScript. The same method name serves
                // both the fire-and-forget call (`invoke`) and the async-result
                // call (`invoke_typed`). When Rust calls `invoke`, it ignores the
                // returned `WhiskerValue`; when it calls `invoke_typed` it awaits
                // the `.map(["value": ...])` result. Returning the value here
                // covers both paths without any branching.
                //
                // Args: `["<js>"]` (positional).
                Function("evaluateJavaScript") { (view: WhiskerWebViewView, args: [WhiskerValue]) -> WhiskerValue in
                    guard let script = args.first?.asString else {
                        return .map(["value": .string("")])
                    }
                    return view.evaluateJavaScript(script)
                }

                // Returns `.bool(webView.canGoBack)` for the async-result
                // `invoke_typed::<bool>` path.
                Function("canGoBack") { (view: WhiskerWebViewView, _: [WhiskerValue]) -> WhiskerValue in
                    return view.canGoBackResult()
                }
                Function("canGoForward") { (view: WhiskerWebViewView, _: [WhiskerValue]) -> WhiskerValue in
                    return view.canGoForwardResult()
                }
            }
        }
    }
}
