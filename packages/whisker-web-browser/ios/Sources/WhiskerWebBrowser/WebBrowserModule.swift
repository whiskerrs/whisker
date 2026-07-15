// `whisker-web-browser` Module (iOS).
//
// View-less module mirroring Expo's `expo-web-browser`: `openBrowser`
// (SFSafariViewController, no cookie sharing) and `openAuthSession`
// (ASWebAuthenticationSession, shares cookies + handles the OAuth
// redirect). Both are fire-and-forget `Function`s that report their
// result later via `sendEvent("authSessionCompleted"/"browserClosed", ...)`
// — Whisker's native `Function` dispatch is sync-only; see
// `docs/module-api-design.md` Shape 4.
//
// `ASWebAuthenticationSession` needs no `CFBundleURLTypes`
// registration — it intercepts the redirect before the OS would
// route it, as long as `callbackURLScheme` matches the redirect
// URI's scheme.

import AuthenticationServices
import Foundation
import SafariServices
import UIKit
import WhiskerModule

private final class PresentationAnchor: NSObject, ASWebAuthenticationPresentationContextProviding {
    func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        WebBrowserModule.keyWindow() ?? ASPresentationAnchor()
    }
}

public final class WebBrowserModule: Module {
    private var authSession: ASWebAuthenticationSession?
    private var safari: SFSafariViewController?
    private let anchor = PresentationAnchor()

    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("WebBrowser")
            Events("authSessionCompleted", "browserClosed")

            Function("openAuthSession") { [weak self] (args: [WhiskerValue]) -> WhiskerValue in
                guard let url = args.first?.asString.flatMap(URL.init),
                    let redirectUrl = args.count > 1 ? args[1].asString : nil
                else { return .null }
                let ephemeral = args.count > 2 ? (args[2].asBool ?? false) : false
                let scheme = URL(string: redirectUrl)?.scheme
                DispatchQueue.main.async {
                    self?.startAuthSession(url: url, callbackScheme: scheme, ephemeral: ephemeral)
                }
                return .null
            }

            Function("dismissAuthSession") { [weak self] _ in
                DispatchQueue.main.async {
                    self?.authSession?.cancel()
                    self?.authSession = nil
                }
                return .null
            }

            Function("openBrowser") { [weak self] (args: [WhiskerValue]) -> WhiskerValue in
                guard let url = args.first?.asString.flatMap(URL.init) else { return .null }
                DispatchQueue.main.async {
                    self?.openBrowser(url: url)
                }
                return .null
            }

            Function("dismissBrowser") { [weak self] _ in
                DispatchQueue.main.async {
                    self?.safari?.dismiss(animated: true)
                    self?.safari = nil
                    self?.sendEvent("browserClosed", .map(["type": .string("dismiss")]))
                }
                return .null
            }
        }
    }

    private func startAuthSession(url: URL, callbackScheme: String?, ephemeral: Bool) {
        let session = ASWebAuthenticationSession(url: url, callbackURLScheme: callbackScheme) {
            [weak self] callbackUrl, error in
            self?.authSession = nil
            if let callbackUrl = callbackUrl {
                self?.sendEvent(
                    "authSessionCompleted",
                    .map(["type": .string("success"), "url": .string(callbackUrl.absoluteString)])
                )
                return
            }
            if let error = error as? ASWebAuthenticationSessionError,
                error.code == .canceledLogin
            {
                self?.sendEvent("authSessionCompleted", .map(["type": .string("cancel")]))
                return
            }
            self?.sendEvent(
                "authSessionCompleted",
                .map([
                    "type": .string("error"),
                    "message": .string(error?.localizedDescription ?? "unknown error"),
                ])
            )
        }
        session.presentationContextProvider = anchor
        session.prefersEphemeralWebBrowserSession = ephemeral
        authSession = session
        session.start()
    }

    private func openBrowser(url: URL) {
        guard let presenter = Self.keyWindow()?.rootViewController else { return }
        let vc = SFSafariViewController(url: url)
        safari = vc
        presenter.present(vc, animated: true)
    }

    fileprivate static func keyWindow() -> UIWindow? {
        for scene in UIApplication.shared.connectedScenes {
            guard let windowScene = scene as? UIWindowScene,
                windowScene.activationState == .foregroundActive
            else { continue }
            if let key = windowScene.windows.first(where: { $0.isKeyWindow }) {
                return key
            }
        }
        return UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .flatMap { $0.windows }
            .first { $0.isKeyWindow }
    }
}
