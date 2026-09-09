import UIKit
@testable import WhiskerModule
@testable import WhiskerRuntime
import XCTest

@MainActor
final class ParagraphTests: XCTestCase {
    func testTopAndBottomAttachmentsShareOneLineHeight() throws {
        let (source, value) = try fixture("vertical-alignment")
        let paragraph = try WhiskerParagraph(value: value, text: source)
        let (layout, _) = paragraph.prepare(attributes: [.font: UIFont.systemFont(ofSize: 16)], width: 400, maxLines: 0, overflow: .clip)
        XCTAssertGreaterThanOrEqual(layout.size.height, 80)
        XCTAssertLessThan(layout.size.height, 90)
        guard case .array(let placements) = layout.inlinePlacements(paragraph.attachments),
              case .map(let top) = placements[0], case .array(let a) = top["origin"],
              case .map(let bottom) = placements[1], case .array(let b) = bottom["origin"] else { return XCTFail("missing attachments") }
        XCTAssertEqual(try XCTUnwrap(a[1].asDouble) + 80, try XCTUnwrap(b[1].asDouble) + 50, accuracy: 1)
    }

    func testTruncationTokenIsOutsideSourceAndOnlyPlacedOnOverflow() throws {
        let (source, value) = try fixture("truncation")
        let paragraph = try WhiskerParagraph(value: value, text: source)
        let (wide, full) = paragraph.prepare(attributes: [.font: UIFont.systemFont(ofSize: 16)], width: 1200, maxLines: 1, overflow: .clip)
        XCTAssertNil(full.visibleEnd)
        guard case .array(let hidden) = wide.inlinePlacements(full.attachments), case .map(let item) = hidden.first else { return XCTFail("missing placement") }
        XCTAssertEqual(item["origin"], .null)
        for width in [120.0, 20.0] {
            let (layout, clipped) = paragraph.prepare(attributes: [.font: UIFont.systemFont(ofSize: 16)], width: width, maxLines: 1, overflow: .clip)
            let end = try XCTUnwrap(clipped.visibleEnd)
            XCTAssertLessThan(end, (source as NSString).length)
            XCTAssertLessThanOrEqual(layout.size.width, width)
            let last = layout.storage.length - 1
            XCTAssertNotNil(layout.storage.attribute(.attachment, at: last, effectiveRange: nil))
            XCTAssertEqual(clipped.copyText(NSRange(location: 0, length: layout.storage.length)), (source as NSString).substring(to: end))
            guard case .map(let geometry) = layout.measurementGeometry(clipped), case .array(let lines) = geometry["lines"], case .map(let line) = lines.last else { return XCTFail("missing geometry") }
            XCTAssertEqual(lines.count, 1)
            XCTAssertEqual(line["end"], .int(Int64((source as NSString).length)))
            XCTAssertEqual(line["ellipsis"], .int(Int64((source as NSString).length - end)))
        }
    }

    func testMixedFontsUseTheSameAttributedContentForMeasurementAndPaint() throws {
        let (text, value) = try fixture()
        let paragraph = try WhiskerParagraph(value: value, text: text)
        let attributed = paragraph.attributedString(text, attributes: [.font: UIFont.systemFont(ofSize: 16)])
        XCTAssertEqual((attributed.attribute(.font, at: 9, effectiveRange: nil) as? UIFont)?.pointSize, 36)
        XCTAssertEqual(attributed.attribute(.foregroundColor, at: 9, effectiveRange: nil) as? UIColor, .red)
        XCTAssertEqual((attributed.attribute(.font, at: 8, effectiveRange: nil) as? UIFont)?.pointSize, 16)
        let narrow = WhiskerParagraphLayout(text: attributed, width: 130, maxLines: 0, overflow: .clip)
        let wide = WhiskerParagraphLayout(text: attributed, width: 600, maxLines: 0, overflow: .clip)
        XCTAssertGreaterThan(narrow.size.height, wide.size.height)
        XCTAssertGreaterThan(wide.baselines.first, 20)
        XCTAssertLessThanOrEqual(narrow.size.width, 130)
        let image = UIGraphicsImageRenderer(size: CGSize(width: 140, height: narrow.size.height + 10)).image { _ in
            narrow.draw(at: CGPoint(x: 5, y: 5))
        }
        XCTAssertNotNil(image.cgImage)
    }

    func testRejectsRangesInsideAnEmojiSurrogatePair() throws {
        let (text, value) = try fixture()
        guard case .map(var fields) = value,
              case .array(var runs) = fields["runs"],
              case .map(var first) = runs[0] else { return XCTFail("invalid fixture") }
        first["end"] = .int(7)
        runs[0] = .map(first)
        fields["runs"] = .array(runs)
        XCTAssertThrowsError(try WhiskerParagraph(value: .map(fields), text: text))
    }

    func testInlineAttachmentWrapsAsAnAtomicMeasuredBox() throws {
        let (text, value) = try fixture("attachment")
        let paragraph = try WhiskerParagraph(value: value, text: text)
        let attributed = paragraph.attributedString(text, attributes: [.font: UIFont.systemFont(ofSize: 16)])
        let wide = WhiskerParagraphLayout(text: attributed, width: 300, maxLines: 0, overflow: .clip)
        let narrow = WhiskerParagraphLayout(text: attributed, width: 60, maxLines: 0, overflow: .clip)
        XCTAssertGreaterThanOrEqual(wide.size.height, 50)
        XCTAssertGreaterThan(narrow.size.height, wide.size.height)
        guard case .array(let placements) = wide.inlinePlacements(paragraph.attachments),
              case .map(let placement) = placements.first,
              case .array(let origin) = placement["origin"] else { return XCTFail("missing inline placement") }
        XCTAssertEqual(placement["node"], .int(22))
        XCTAssertEqual(try XCTUnwrap(origin[1].asDouble) + 37, Double(wide.baselines.first), accuracy: 0.5)
        let truncated = WhiskerParagraphLayout(text: attributed, width: 60, maxLines: 1, overflow: .ellipsis)
        guard case .array(let hidden) = truncated.inlinePlacements(paragraph.attachments),
              case .map(let item) = hidden.first else { return XCTFail("missing hidden placement") }
        XCTAssertEqual(item["origin"], .null)
    }

    func testRoundedInlineBackgroundUsesFragmentGeometry() throws {
        let (text, value) = try fixture("rounded-background")
        let paragraph = try WhiskerParagraph(value: value, text: text)
        let layout = WhiskerParagraphLayout(text: paragraph.attributedString(text, attributes: [.font: UIFont.systemFont(ofSize: 36)]), width: 300, maxLines: 0, overflow: .clip)
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        let image = UIGraphicsImageRenderer(size: CGSize(width: 300, height: 100), format: format).image { context in
            UIColor.white.setFill()
            context.fill(CGRect(x: 0, y: 0, width: 300, height: 100))
            layout.draw(at: CGPoint(x: 5, y: 5))
        }
        let cgImage = try XCTUnwrap(image.cgImage)
        var pixels = [UInt8](repeating: 0, count: 300 * 100 * 4)
        pixels.withUnsafeMutableBytes { bytes in
            let context = CGContext(data: bytes.baseAddress, width: 300, height: 100, bitsPerComponent: 8, bytesPerRow: 300 * 4, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
            context.draw(cgImage, in: CGRect(x: 0, y: 0, width: 300, height: 100))
        }
        func isRed(_ x: Int, _ y: Int) -> Bool {
            let index = (y * 300 + x) * 4
            return pixels[index] > 200 && pixels[index + 1] < 30 && pixels[index + 2] < 30
        }
        let red = (0..<100).flatMap { y in (0..<300).filter { isRed($0, y) }.map { CGPoint(x: $0, y: y) } }
        XCTAssertFalse(red.isEmpty)
        let left = Int(try XCTUnwrap(red.map(\.x).min()))
        let right = Int(try XCTUnwrap(red.map(\.x).max()))
        let top = Int(try XCTUnwrap(red.map(\.y).min()))
        let bottom = Int(try XCTUnwrap(red.map(\.y).max()))
        XCTAssertTrue(isRed((left + right) / 2, (top + bottom) / 2))
        XCTAssertFalse(isRed(left + 1, top + 1))
    }

    private func fixture(_ name: String = "styled") throws -> (String, WhiskerValue) {
        var root = URL(fileURLWithPath: #filePath)
        for _ in 0..<5 { root.deleteLastPathComponent() }
        let data = try Data(contentsOf: root.appendingPathComponent("tests/host-conformance/paragraphs/\(name).json"))
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
        return (try XCTUnwrap(json["text"] as? String), .from(nsObject: try XCTUnwrap(json["paragraph"])))
    }
}

extension ParagraphTests {
    func testInlineAccessibilityActionsFollowVisibility() throws {
        let (text, value) = try fixture()
        guard case .map(var fields) = value, case .array(var runs) = fields["runs"], case .map(var run) = runs[0] else { return XCTFail("fixture") }
        run["action"] = .int(42)
        runs[0] = .map(run)
        fields["runs"] = .array(runs)
        let paragraph = try WhiskerParagraph(value: .map(fields), text: text)
        let label = WhiskerTextLabel(frame: .zero)
        label.richParagraph = paragraph
        label.preparedContent = 7
        var events: [WhiskerValue] = []
        label.installWhiskerEventSink { name, value in if name == "textactivate" { events.append(value) } }
        let action = try XCTUnwrap(label.accessibilityCustomActions?.first)
        XCTAssertTrue(try XCTUnwrap(action.actionHandler)(action))
        XCTAssertEqual(events, [.map(["span": .int(42), "revision": .int(7)])])
        label.preparedContent = 8
        XCTAssertFalse(try XCTUnwrap(action.actionHandler)(action))
        label.richParagraph = paragraph.withVisibleEnd(0)
        XCTAssertTrue(label.accessibilityCustomActions?.isEmpty == true)
        XCTAssertEqual(label.accessibilityLabel, "")
    }

    func testPreparationLeaseRetainsLayoutUntilCacheRelease() {
        let cache = PreparedParagraphs()
        var layout: WhiskerParagraphLayout? = WhiskerParagraphLayout(text: NSAttributedString(string: "kept", attributes: [.font: UIFont.systemFont(ofSize: 16)]), width: 100, maxLines: 0, overflow: .clip)
        weak var retained = layout
        var lease: PreparedParagraph? = cache.insert(layout!, id: 42)
        XCTAssertTrue(cache.layout(42) === layout)
        layout = nil
        XCTAssertNotNil(retained)
        XCTAssertNotNil(lease)
        lease = nil
        XCTAssertNil(cache.layout(42))
        XCTAssertNil(retained)
    }
}
