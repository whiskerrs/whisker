#if canImport(UIKit)
import UIKit
@testable import WhiskerModule
@testable import WhiskerRuntime
import XCTest

@MainActor
final class TextMeasurementTests: XCTestCase {
    func testConstraintVariantsReuseParagraphWithIndependentResponses() throws {
        let view = WhiskerView(frame: .zero)
        let fixture = try paragraph("styled")
        let natural = try measure(view, fixture: fixture)
        let fixedWidth = try measure(view, fixture: fixture) { $0.known_mask = 1; $0.known_width = 350 }
        let zeroHeight = try measure(view, fixture: fixture) { $0.known_mask = 2; $0.known_height = 0 }
        let fixedHeight = try measure(view, fixture: fixture) { $0.known_mask = 2; $0.known_height = natural.raw.height }
        let layout = try XCTUnwrap(view.preparedParagraphs.layout(natural.raw.key))
        for result in [fixedWidth, zeroHeight, fixedHeight] {
            XCTAssertTrue(view.preparedParagraphs.layout(result.raw.key) === layout)
            XCTAssertEqual(result.geometry, natural.geometry)
            XCTAssertEqual(result.raw.first_baseline, natural.raw.first_baseline)
            XCTAssertEqual(result.raw.last_baseline, natural.raw.last_baseline)
        }
        XCTAssertEqual(fixedWidth.raw.width, 350)
        XCTAssertEqual(fixedWidth.raw.height, natural.raw.height)
        XCTAssertEqual(zeroHeight.raw.width, natural.raw.width)
        XCTAssertEqual(zeroHeight.raw.height, 0)
        XCTAssertEqual(fixedHeight.raw.height, natural.raw.height)
    }

    func testDifferentNodesAndLayoutInputsDoNotShareParagraphs() throws {
        let fixture = try paragraph("styled")
        let changes: [(inout WhiskerMobileMeasureRequest) -> Void] = [
            { $0.node = 2 }, { $0.environment_epoch = 2 },
            { $0.available_width = 100 }, { $0.known_mask = 1; $0.known_width = 100 },
            { $0.font_size = 30 }, { $0.line_height = 50 },
            { $0.word_break = 1 }, { $0.direction = 1 }, { $0.alignment = 3 },
            { $0.indent_percentage = 0.1 }, { $0.max_lines = 1; $0.overflow = 1 },
        ]
        for change in changes {
            let view = WhiskerView(frame: .zero)
            let first = try measure(view, fixture: fixture)
            let second = try measure(view, fixture: fixture, change: change)
            XCTAssertFalse(view.preparedParagraphs.layout(first.raw.key) === view.preparedParagraphs.layout(second.raw.key))
            let uncached = try measure(WhiskerView(frame: .zero), fixture: fixture, change: change)
            assertEqual(second, uncached)
        }
    }

    func testAttachmentAndTruncationGeometryMatchesFreshMeasurement() throws {
        for name in ["attachment", "truncation", "vertical-alignment"] {
            let fixture = try paragraph(name)
            let view = WhiskerView(frame: .zero)
            let first = try measure(view, fixture: fixture) { $0.available_width = 120; $0.max_lines = 1 }
            let repeated = try measure(view, fixture: fixture) { $0.available_width = 120; $0.max_lines = 1; $0.known_mask = 2; $0.known_height = 37 }
            let fresh = try measure(WhiskerView(frame: .zero), fixture: fixture) { $0.available_width = 120; $0.max_lines = 1; $0.known_mask = 2; $0.known_height = 37 }
            XCTAssertTrue(view.preparedParagraphs.layout(first.raw.key) === view.preparedParagraphs.layout(repeated.raw.key))
            assertEqual(repeated, fresh)
        }
    }

    func testChangedTextAndAttachmentPayloadInvalidatePreparation() throws {
        let styled = try paragraph("styled")
        let view = WhiskerView(frame: .zero)
        let original = try measure(view, fixture: styled)
        let changedText = (styled.0.replacingOccurrences(of: "Small", with: "Other"), styled.1)
        let changed = try measure(view, fixture: changedText)
        XCTAssertFalse(view.preparedParagraphs.layout(original.raw.key) === view.preparedParagraphs.layout(changed.raw.key))
        assertEqual(changed, try measure(WhiskerView(frame: .zero), fixture: changedText))

        let attachment = try paragraph("attachment")
        let first = try measure(view, fixture: attachment)
        guard case .map(var fields) = attachment.1,
              case .array(var attachments) = fields["attachments"],
              case .map(var item) = attachments.first else { return XCTFail("missing attachment") }
        item["width"] = .float(150)
        attachments[0] = .map(item)
        fields["attachments"] = .array(attachments)
        let resized: (String, WhiskerValue?) = (attachment.0, .map(fields))
        let second = try measure(view, fixture: resized)
        XCTAssertFalse(view.preparedParagraphs.layout(first.raw.key) === view.preparedParagraphs.layout(second.raw.key))
        assertEqual(second, try measure(WhiskerView(frame: .zero), fixture: resized))
    }

    func testPresentationEndsReuseWithoutReleasingMeasurementLeases() throws {
        let view = WhiskerView(frame: .zero)
        let fixture = try paragraph("styled")
        let first = try measure(view, fixture: fixture)
        let layout = try XCTUnwrap(view.preparedParagraphs.layout(first.raw.key))
        var response = WhiskerMobileApplyResponse()
        _ = view.applyFrame(WhiskerMobileFrame(), response: &response)
        let second = try measure(view, fixture: fixture)
        XCTAssertFalse(view.preparedParagraphs.layout(second.raw.key) === layout)
        XCTAssertTrue(view.preparedParagraphs.layout(first.raw.key) === layout)
        assertEqual(first, second)
    }

    func testPlainConstraintVariantsMatchFreshMeasurement() throws {
        let view = WhiskerView(frame: .zero)
        let fixture: (String, WhiskerValue?) = ("日本語の長い文章と English text wrapping across several lines.", nil)
        let changes: [(inout WhiskerMobileMeasureRequest) -> Void] = [
            { _ in }, { $0.known_mask = 2; $0.known_height = 0 },
            { $0.word_break = 2 }, { $0.max_lines = 1 },
            { $0.available_width_kind = 1 }, { $0.available_width_kind = 2 },
        ]
        for change in changes {
            let actual = try measure(view, fixture: fixture, change: change)
            let expected = try measure(WhiskerView(frame: .zero), fixture: fixture, change: change)
            assertEqual(actual, expected)
        }
    }

    func testPlainTextLeasesTheLayoutItWasMeasuredWith() throws {
        let view = WhiskerView(frame: .zero)
        let result = try measure(view, fixture: ("日本語の長い文章と English text wrapping across several lines.", nil))
        let layout = try XCTUnwrap(view.preparedParagraphs.layout(result.raw.key))
        XCTAssertEqual(result.raw.width, Float(ceil(layout.size.width)))
        XCTAssertEqual(result.raw.height, Float(ceil(layout.size.height)))
        XCTAssertEqual(result.raw.first_baseline, Float(layout.baselines.first))
        XCTAssertGreaterThan(result.raw.last_baseline, result.raw.first_baseline)
    }

    func testLabelAlignsInstalledPlainLayoutInsideWiderBounds() throws {
        let text = NSAttributedString(string: "Hi", attributes: [.font: UIFont.systemFont(ofSize: 16), .foregroundColor: UIColor.black])
        // A larger measured font tells the installed layout's glyphs apart from UILabel's own.
        let measured = NSAttributedString(string: "Hi", attributes: [.font: UIFont.systemFont(ofSize: 32)])
        let layout = WhiskerParagraphLayout(text: measured, width: .greatestFiniteMagnitude, maxLines: 0, overflow: .clip)
        func ink(_ alignment: NSTextAlignment) -> (center: CGFloat, width: CGFloat) {
            let label = WhiskerTextLabel(frame: CGRect(x: 0, y: 0, width: 200, height: ceil(layout.size.height)))
            label.textAlignment = alignment
            label.attributedText = text
            label.installPreparedParagraph(layout)
            let image = UIGraphicsImageRenderer(bounds: label.bounds).image { label.layer.render(in: $0.cgContext) }
            let pixels = try! XCTUnwrap(image.cgImage?.dataProvider?.data as Data?)
            let width = image.cgImage!.width, row = image.cgImage!.bytesPerRow
            var inked: [Int] = []
            for x in 0..<width where (0..<image.cgImage!.height).contains(where: { pixels[$0 * row + x * 4 + 3] > 0 }) {
                inked.append(x)
            }
            let span = CGFloat((inked.last ?? 0) - (inked.first ?? 0)) / image.scale
            return (CGFloat(inked.reduce(0, +)) / CGFloat(max(inked.count, 1)) / image.scale, span)
        }
        XCTAssertGreaterThan(ink(.left).width, layout.size.width * 0.8)
        XCTAssertLessThan(ink(.left).center, layout.size.width)
        XCTAssertEqual(ink(.center).center, 100, accuracy: 4)
        XCTAssertGreaterThan(ink(.right).center, 200 - layout.size.width)
    }

    func testPresentationReleasesUnleasedCachedLayouts() throws {
        let view = WhiskerView(frame: .zero)
        let fixture = try paragraph("styled")
        weak var layout: WhiskerParagraphLayout?
        try autoreleasepool {
            let result = try measure(view, fixture: fixture)
            layout = view.preparedParagraphs.layout(result.raw.key)
            XCTAssertNotNil(layout)
        }
        XCTAssertNotNil(layout)
        var response = WhiskerMobileApplyResponse()
        _ = view.applyFrame(WhiskerMobileFrame(), response: &response)
        XCTAssertNil(layout)
    }

    func testOwnedStyleInputsInvalidateCachedPlainMetrics() throws {
        let fixture: (String, WhiskerValue?) = ("A long line of text 日本語 that wraps and has a baseline.", nil)
        let changes: [(inout WhiskerMobileMeasureRequest) -> Void] = [
            { $0.font_size = 24 }, { $0.font_weight = 700 }, { $0.font_style = 1 },
            { $0.letter_spacing = 3 }, { $0.line_height = 40 },
            { $0.indent_logical_pixels = 20 }, { $0.indent_percentage = 10 },
            { $0.direction = 2 }, { $0.alignment = 4 },
            { $0.font_optical_sizing = 1 }, { $0.environment_epoch = 2 },
        ]
        for change in changes {
            let view = WhiskerView(frame: .zero)
            _ = try measure(view, fixture: fixture)
            let cached = try measure(view, fixture: fixture, change: change)
            let fresh = try measure(WhiskerView(frame: .zero), fixture: fixture, change: change)
            assertEqual(cached, fresh)
        }
    }

    func testCacheOwnsFontFeaturesAndVariations() {
        var request = WhiskerMobileMeasureRequest()
        request.font_size = 16
        var feature = WhiskerMobileFontFeature()
        feature.tag = (108, 105, 103, 97) // liga
        feature.value = 1
        var variation = WhiskerMobileFontVariation()
        variation.tag = (119, 103, 104, 116) // wght
        variation.value = 400
        let original = withUnsafePointer(to: &feature) { featurePointer in
            withUnsafePointer(to: &variation) { variationPointer in
                request.font_features = featurePointer
                request.font_feature_count = 1
                request.font_variations = variationPointer
                request.font_variation_count = 1
                return TextMeasurementCache.Style(request, families: ["system"], widthBasis: 100)
            }
        }
        feature.value = 0
        variation.value = 700
        let changed = withUnsafePointer(to: &feature) { featurePointer in
            withUnsafePointer(to: &variation) { variationPointer in
                request.font_features = featurePointer
                request.font_variations = variationPointer
                return TextMeasurementCache.Style(request, families: ["system"], widthBasis: 100)
            }
        }
        XCTAssertNotEqual(original, changed)
        XCTAssertEqual(original.features.first?.value, 1)
        XCTAssertEqual(original.variations.first?.value, 400)
    }

    func testOneFrameReusesEveryEntryItBuilt() {
        var request = WhiskerMobileMeasureRequest()
        request.font_size = 16
        let input = TextMeasurementCache.Input(
            source: "text", payload: nil,
            style: TextMeasurementCache.Style(request, families: ["system"], widthBasis: 100),
            width: 100, maxLines: 0, overflow: 0, wordBreak: 0, environmentEpoch: 0
        )
        let cache = TextMeasurementCache()
        var builds = 0
        for _ in 0..<2 {
            for node in 0..<1_000 {
                _ = cache.plain(node: UInt64(node), input: input, probe: false) {
                    builds += 1
                    return TextMeasurementCache.Plain(layout: nil, size: .zero, firstBaseline: 0, lastBaseline: 0)
                }
            }
        }
        XCTAssertEqual(builds, 1_000)
    }

    func testPlainLayoutServesEveryWidthItsLinesFit() throws {
        let view = WhiskerView(frame: .zero)
        let fixture: (String, WhiskerValue?) = ("One short line", nil)
        let widest = try measure(view, fixture: fixture) { $0.available_width_kind = 2 }
        let layout = try XCTUnwrap(view.preparedParagraphs.layout(widest.raw.key))
        let fitting = try measure(view, fixture: fixture) { $0.available_width = widest.raw.width }
        XCTAssertTrue(view.preparedParagraphs.layout(fitting.raw.key) === layout)
        assertEqual(fitting, widest)
        let narrow: (inout WhiskerMobileMeasureRequest) -> Void = { $0.available_width = widest.raw.width / 2 }
        let wrapped = try measure(view, fixture: fixture, change: narrow)
        XCTAssertFalse(view.preparedParagraphs.layout(wrapped.raw.key) === layout)
        assertEqual(wrapped, try measure(WhiskerView(frame: .zero), fixture: fixture, change: narrow))
        let wide = try measure(view, fixture: fixture) { $0.available_width = 2_000 }
        assertEqual(wide, widest)

        let long: (String, WhiskerValue?) = ("Several words that wrap onto more than one line at this width.", nil)
        let lines = try measure(view, fixture: long) { $0.node = 2; $0.available_width = 200 }
        let tighter: (inout WhiskerMobileMeasureRequest) -> Void = { $0.node = 2; $0.available_width = lines.raw.width }
        let same = try measure(view, fixture: long, change: tighter)
        XCTAssertLessThan(lines.raw.width, 200)
        XCTAssertTrue(view.preparedParagraphs.layout(same.raw.key) === view.preparedParagraphs.layout(lines.raw.key))
        assertEqual(same, try measure(WhiskerView(frame: .zero), fixture: long, change: tighter))
    }

    func testMinContentProbeIsMeasuredOncePerTextAndStyle() throws {
        let view = WhiskerView(frame: .zero)
        let fixture: (String, WhiskerValue?) = ("Words to break", nil)
        let probe: (inout WhiskerMobileMeasureRequest) -> Void = { $0.available_width_kind = 1 }
        let first = try measure(view, fixture: fixture, change: probe)
        XCTAssertNil(view.preparedParagraphs.layout(first.raw.key))
        var response = WhiskerMobileApplyResponse()
        _ = view.applyFrame(WhiskerMobileFrame(), response: &response)
        assertEqual(try measure(view, fixture: fixture) { $0.available_width_kind = 1; $0.node = 2 }, first)
        assertEqual(try measure(view, fixture: fixture) { $0.available_width_kind = 1; $0.font_size = 32 },
            try measure(WhiskerView(frame: .zero), fixture: fixture) { $0.available_width_kind = 1; $0.font_size = 32 })

        var request = WhiskerMobileMeasureRequest()
        request.font_size = 16
        let input = TextMeasurementCache.Input(
            source: "text", payload: nil,
            style: TextMeasurementCache.Style(request, families: ["system"], widthBasis: 0),
            width: 1, maxLines: 0, overflow: 0, wordBreak: 0, environmentEpoch: 0
        )
        let cache = TextMeasurementCache()
        var builds = 0
        for node in 0..<3 {
            _ = cache.plain(node: UInt64(node), input: input, probe: true) {
                builds += 1
                return TextMeasurementCache.Plain(layout: nil, size: .zero, firstBaseline: 0, lastBaseline: 0)
            }
            cache.removeAll()
        }
        XCTAssertEqual(builds, 1)
    }

    private var nextKey: UInt64 = 0

    private func measure(_ view: WhiskerView, fixture: (String, WhiskerValue?), change: (inout WhiskerMobileMeasureRequest) -> Void = { _ in }) throws -> Measurement {
        nextKey += 1
        var request = WhiskerMobileMeasureRequest()
        request.key = nextKey
        request.node = 1
        request.kind = UInt32(WHISKER_MEASURE_TEXT)
        request.environment_epoch = 1
        request.available_width = 350
        request.available_height_kind = 2
        request.wrap = 1
        request.font_size = 16
        request.font_weight = 400
        change(&request)
        var payload = (fixture.1 ?? .null).toRaw()
        defer { WhiskerValue.releaseRaw(&payload) }
        var response = WhiskerMobileMeasureResponse()
        let bytes = fixture.0.utf8.map { CChar(bitPattern: $0) }
        let accepted = "system".withCString { familyBytes in
            var family = WhiskerStringRef(ptr: familyBytes, len: 6)
            return withUnsafePointer(to: &family) { familyPointer in
                request.font_families = familyPointer
                request.font_family_count = 1
                return bytes.withUnsafeBufferPointer { text in
                    withUnsafePointer(to: &payload) { paragraph in
                        request.text = WhiskerStringRef(ptr: text.baseAddress, len: text.count)
                        request.paragraph = fixture.1 == nil ? nil : paragraph
                        return whiskerIOSMeasure(Unmanaged.passUnretained(view).toOpaque(), &request, 1, &response)
                    }
                }
            }
        }
        XCTAssertTrue(accepted)
        XCTAssertEqual(response.status, UInt32(WHISKER_MEASURE_READY))
        return Measurement(response)
    }

    private func paragraph(_ name: String) throws -> (String, WhiskerValue?) {
        var root = URL(fileURLWithPath: #filePath)
        for _ in 0..<5 { root.deleteLastPathComponent() }
        let data = try Data(contentsOf: root.appendingPathComponent("tests/host-conformance/paragraphs/\(name).json"))
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
        return (try XCTUnwrap(json["text"] as? String), .from(nsObject: try XCTUnwrap(json["paragraph"])))
    }

    private func assertEqual(_ actual: Measurement, _ expected: Measurement, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertEqual(actual.raw.width, expected.raw.width, file: file, line: line)
        XCTAssertEqual(actual.raw.height, expected.raw.height, file: file, line: line)
        XCTAssertEqual(actual.raw.first_baseline, expected.raw.first_baseline, file: file, line: line)
        XCTAssertEqual(actual.raw.last_baseline, expected.raw.last_baseline, file: file, line: line)
        XCTAssertEqual(actual.geometry, expected.geometry, file: file, line: line)
    }
}

private final class Measurement {
    var raw: WhiskerMobileMeasureResponse
    var geometry: WhiskerValue? { raw.paragraph.map { .from(raw: $0.pointee) } }

    init(_ raw: WhiskerMobileMeasureResponse) { self.raw = raw }

    deinit {
        raw.release_paragraph?(raw.paragraph)
        raw.release_prepared_layout?(raw.prepared_layout)
    }
}
#endif
