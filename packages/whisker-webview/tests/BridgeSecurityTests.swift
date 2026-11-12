import XCTest
import WebKit
import whisker_webview
import WhiskerModule

final class BridgeSecurityTests: XCTestCase {
    @MainActor func testOriginAndFrameRestrictions() async throws {
        let view = WhiskerWebViewView(frame: .zero)
        let web = try XCTUnwrap(view.view().subviews.compactMap { $0 as? WKWebView }.first)
        var received: [String] = []
        let main = expectation(description: "allowed main frame")
        view.installWhiskerEventSink { name, detail in
            guard name == "message", case .map(let fields) = detail,
                  case .string(let text) = fields["data"] else { return }
            received.append(text)
            if text == "main" { main.fulfill() }
        }
        view.setJavascriptEnabled(true)
        view.setMessageOrigins("[\"https://allowed.example\"]")
        web.loadHTMLString("""
          <html><body><script>setTimeout(function(){window.whisker.postMessage('main');},500);</script>
          <iframe srcdoc="<script>setTimeout(function(){window.webkit.messageHandlers.whisker.postMessage('iframe');},500);</script>"></iframe>
          </body></html>
          """, baseURL: URL(string: "https://allowed.example"))
        await fulfillment(of: [main], timeout: 20)
        try await Task.sleep(nanoseconds: 1000000000)
        XCTAssertEqual(received, ["main"])
        web.loadHTMLString("<script>setTimeout(function(){window.webkit.messageHandlers.whisker.postMessage('evil');},100);</script>", baseURL: URL(string: "https://evil.example"))
        try await Task.sleep(nanoseconds: 2000000000)
        XCTAssertEqual(received, ["main"])
    }
}
