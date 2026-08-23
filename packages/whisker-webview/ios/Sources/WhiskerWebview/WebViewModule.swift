// `whisker-webview` ModuleDefinition (iOS).
//
// The codegen plugin discovers this `Module` subclass and emits a
// registration block in `WhiskerWebView+Generated.swift` that registers
// `definitionLazy.view!.viewClass` with `LynxComponentRegistry` under
// "whisker-webview:WebView", then calls `module.registerWithLynx()` so
// every `Prop(...)` setter and `Function(...)` method installs via the
// Obj-C-runtime path.
//
// The `WhiskerWebViewView` Lynx UI subclass lives in `WhiskerWebView.swift`.
//
// ## Prop delivery
//
// Bool props and the JSON-array whitelist are pre-stringified by the Rust
// layer, so every prop reads through `value.asString`.

import WhiskerModule

@WhiskerModule
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

                // Declaration-only metadata for the codegen / docs scanner;
                // dispatch happens in `WhiskerWebView.swift`.
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
                // The result-returning methods still use the sync `Function`
                // form: Lynx's `<name>:withResult:` dispatch calls the
                // closure and hands the returned `WhiskerValue` straight to
                // the Rust-side `invoke_typed` awaiter.

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

                Function("postMessage") { (view: WhiskerWebViewView, args: [WhiskerValue]) -> WhiskerValue in
                    if let data = args.first?.asString {
                        view.postMessageToPage(data)
                    }
                    return .null
                }

                // One method name serves both `invoke` (ignores the return)
                // and `invoke_typed` (awaits the `.map(["value": ...])`), so
                // returning the value covers both without branching.
                Function("evaluateJavaScript") { (view: WhiskerWebViewView, args: [WhiskerValue]) -> WhiskerValue in
                    guard let script = args.first?.asString else {
                        return .map(["value": .string("")])
                    }
                    return view.evaluateJavaScript(script)
                }

                // Async so the WKWebView completion can carry the JS
                // result back through the promise.
                AsyncFunction("evaluateJavaScriptWithResult") { (view: WhiskerWebViewView, args: [WhiskerValue], promise: WhiskerPromise) in
                    guard let script = args.first?.asString else {
                        return promise.reject("evaluateJavaScriptWithResult: missing script argument")
                    }
                    view.evaluateJavaScript(script, resolving: promise)
                }

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
