import UIKit
import WhiskerModule
import XCTest

@MainActor
final class TextSelectionTests: XCTestCase {
    func testSelectionQueriesUseNativeUTF16RangesAndRejectStaleRevision() throws {
        let registration = WhiskerElementRegistration(
            elementType: 2, name: WhiskerBuiltInElements.textName, childPolicy: .richText, measurement: .text,
            properties: [WhiskerPropertyBinding(id: 1, name: "selectable", value: .bool)],
            events: [WhiskerEventBinding(id: 1, name: "selectionchange", detail: .map), WhiskerEventBinding(id: 2, name: "textqueryresult", detail: .map), WhiskerEventBinding(id: 3, name: "textactivate", detail: .map)],
            commands: [WhiskerCommandBinding(id: 1, name: "setSelection", arguments: .map), WhiskerCommandBinding(id: 2, name: "clearSelection", arguments: .map), WhiskerCommandBinding(id: 3, name: "textQuery", arguments: .map)]
        )
        XCTAssertTrue(WhiskerElementRegistry.bind([registration]))
        var replies: [WhiskerValue] = []
        let mounted = try XCTUnwrap(WhiskerElementRegistry.mount(2) { event, detail in
            if event.name == "textqueryresult" { replies.append(detail) }
        })
        mounted.setEventMask(3)
        mounted.view.frame = CGRect(x: 0, y: 0, width: 300, height: 70)
        XCTAssertTrue(mounted.setText(WhiskerTextContent(value: "Hello 🦀 world", preparedContent: 7, fontSize: 20, fontWeight: 400, color: .black)))
        mounted.setProperty(1, value: .bool(true))
        mounted.view.layoutIfNeeded()
        mounted.invokeCommand(1, parameters: .map(["revision": .int(7), "start": .int(6), "end": .int(8)]))
        func request(_ revision: Int64, _ kind: String) {
            mounted.invokeCommand(3, parameters: .map(["id": .int(9), "revision": .int(revision), "kind": .string(kind), "start": .int(6), "end": .int(8)]))
        }
        request(7, "selectedText")
        guard case .map(let selected) = replies.last else { return XCTFail("missing text query response") }
        XCTAssertEqual(selected["text"]?.asString, "🦀")
        request(7, "boundingRects")
        guard case .map(let geometry) = replies.last, case .array(let rects) = geometry["rects"] else { return XCTFail("missing text rectangles") }
        XCTAssertFalse(rects.isEmpty)
        request(8, "selectedText")
        guard case .map(let stale) = replies.last else { return XCTFail("missing stale result") }
        XCTAssertEqual(stale["error"]?.asString, "stale-layout")
        XCTAssertTrue(mounted.setText(WhiskerTextContent(value: "Hello 🦀 world", preparedContent: 7, fontSize: 20, fontWeight: 400, color: .red)))
        request(7, "selectedText")
        guard case .map(let repainted) = replies.last else { return XCTFail("missing repainted selection") }
        XCTAssertEqual(repainted["text"]?.asString, "🦀")
        XCTAssertTrue(mounted.setText(WhiskerTextContent(value: "Changed", preparedContent: 8, fontSize: 20, fontWeight: 400, color: .red)))
        request(8, "selectedText")
        guard case .map(let changed) = replies.last else { return XCTFail("missing cleared selection") }
        XCTAssertEqual(changed["text"]?.asString, "")
    }
}
