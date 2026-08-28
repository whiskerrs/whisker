// `whisker-webview` ModuleDefinition (iOS).
//
// The codegen plugin discovers this `Module` subclass and emits a
// registration block in `WhiskerWebView+Generated.swift` that registers
// `definitionLazy.view!.viewClass` with `LynxComponentRegistry` under
// "whisker-webview:WebView", then calls `module.registerWithLynx()` so
// every `Prop(...)` setter and `Command(...)` handler installs via the
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
            View("whisker-webview:WebView", WhiskerWebViewView.self) {

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

                // ---- One-way View commands -------------------------------

                Command("reload") { (view: WhiskerWebViewView, _: WhiskerValue) in
                    view.reloadPage()
                }
                Command("goBack") { (view: WhiskerWebViewView, _: WhiskerValue) in
                    view.goBackPage()
                }
                Command("goForward") { (view: WhiskerWebViewView, _: WhiskerValue) in
                    view.goForwardPage()
                }
                Command("stopLoading") { (view: WhiskerWebViewView, _: WhiskerValue) in
                    view.stopLoadingPage()
                }

                Command("postMessage") { (view: WhiskerWebViewView, parameters: WhiskerValue) in
                    if let data = parameters.asString {
                        view.postMessageToPage(data)
                    }
                }

                Command("evaluateJavaScript") { (view: WhiskerWebViewView, parameters: WhiskerValue) in
                    guard let script = parameters.asString else {
                        return
                    }
                    view.evaluateJavaScript(script)
                }
            }
        }
    }
}
