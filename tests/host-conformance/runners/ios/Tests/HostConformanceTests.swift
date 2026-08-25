import CoreGraphics
import Foundation
import UIKit
import WhiskerCBridge
@testable import WhiskerRuntime
@testable import WhiskerModule
import XCTest

@MainActor
final class HostConformanceTests: XCTestCase {
    override class func setUp() {
        super.setUp()
        BuiltInElementModule().registerWithWhisker()
    }

    func testEverySharedPaintScenarioUsesProductionUIKitHost() throws {
        let manifest = try json(at: fixtureRoot.appendingPathComponent("manifest.json"))
        let cases = try XCTUnwrap(manifest["cases"] as? [[String: Any]])
        var count = 0
        for entry in cases {
            let relative = try XCTUnwrap(entry["fixture"] as? String)
            let scenario = try json(at: fixtureRoot.appendingPathComponent(relative))
            guard try number(scenario, "schema") == 1,
                  try string(scenario, "id") == string(entry, "id") else {
                throw Failure("manifest and iOS fixture disagree")
            }
            let id = try string(scenario, "id")
            let testSide = try object(scenario, "test")
            guard try array(testSide, "commands").contains(where: {
                try string($0, "type") == "present_box"
            }) else { continue }
            let test = try Driver(id: id).execute(testSide)
            if let reference = scenario["reference"] as? [String: Any] {
                let expected = try Driver(id: id).execute(reference)
                XCTAssertEqual(test.width, expected.width)
                XCTAssertEqual(test.height, expected.height)
                XCTAssertLessThanOrEqual(
                    largestDifference(test.bytes, expected.bytes),
                    1,
                    id
                )
            }
            count += 1
        }
        XCTAssertGreaterThan(count, 0)
    }
}

@MainActor
private final class Driver {
    private let id: String
    private let view = WhiskerView(frame: .zero)
    private var logicalSize = CGSize.zero
    private var checkpoint: Pixels?

    init(id: String) throws {
        self.id = id
        let registration = WhiskerElementRegistration(
            elementType: 1,
            name: WhiskerBuiltInElements.viewName,
            childPolicy: .elements,
            measurement: .none
        )
        guard WhiskerElementRegistry.bind([registration]) else {
            throw Failure("bind built-in UIKit View")
        }
    }

    func execute(_ side: [String: Any]) throws -> Pixels {
        for command in try array(side, "commands") {
            switch try string(command, "type") {
            case "attach_surface":
                logicalSize = CGSize(
                    width: try number(command, "width"),
                    height: try number(command, "height")
                )
            case "present_box":
                try present(command)
            case "checkpoint":
                guard try string(command, "name") == "paint.box" else {
                    throw Failure("unsupported UIKit checkpoint")
                }
                let pixels = try capture()
                checkpoint = pixels
                if let samples = command["samples"] as? [[String: Any]] {
                    try assertPixelSamples(id: id, pixels: pixels, samples: samples)
                }
                if let relations = command["relations"] as? [[String: Any]] {
                    try assertPixelRelations(id: id, pixels: pixels, relations: relations)
                }
            default:
                throw Failure("unsupported UIKit paint command")
            }
        }
        return try unwrap(checkpoint, "paint checkpoint")
    }

    private func present(_ command: [String: Any]) throws {
        let revision = UInt64(try number(command, "revision"))
        let values = try numberArray(command, "rect")
        guard values.count == 4 else { throw Failure("box rect needs four values") }
        var layout = WhiskerMobileLayoutGeometry()
        layout.border = WhiskerMobileRect(
            x: Float(values[0]),
            y: Float(values[1]),
            width: Float(values[2]),
            height: Float(values[3])
        )
        layout.content = WhiskerMobileRect()
        var paint = try boxPaint(command)

        try withUnsafePointer(to: &layout) { layoutPointer in
            try withUnsafePointer(to: &paint) { paintPointer in
                let create = operation(tag: UInt32(WHISKER_OP_CREATE), member: 1)
                let setLayout = operation(
                    tag: UInt32(WHISKER_OP_LAYOUT),
                    payload: UnsafeRawPointer(layoutPointer),
                    count: 1
                )
                let setPaint = operation(
                    tag: UInt32(WHISKER_OP_PAINT),
                    payload: UnsafeRawPointer(paintPointer),
                    count: 1
                )
                var operations = [create, setLayout, setPaint]
                try operations.withUnsafeMutableBufferPointer { buffer in
                    var frame = WhiskerMobileFrame()
                    frame.abi_major = UInt16(WHISKER_MOBILE_ABI_MAJOR)
                    frame.abi_minor = UInt16(WHISKER_MOBILE_ABI_MINOR)
                    frame.protocol_major = 1
                    frame.protocol_minor = 0
                    frame.mode = UInt8(WHISKER_FRAME_SNAPSHOT)
                    frame.surface = 1
                    frame.scene_epoch = 1
                    frame.viewport_epoch = 1
                    frame.frame_id = revision
                    frame.base_revision = 0
                    frame.target_revision = revision
                    if let baseAddress = buffer.baseAddress {
                        frame.operations = UnsafePointer(baseAddress)
                    }
                    frame.operation_count = buffer.count
                    var response = WhiskerMobileApplyResponse()
                    guard view.applyConformanceFrame(frame, response: &response),
                          response.status == UInt8(WHISKER_APPLY_ACCEPTED) else {
                        throw Failure("UIKit Host rejected fixture frame")
                    }
                }
            }
        }
    }

    private func capture() throws -> Pixels {
        guard logicalSize.width > 0, logicalSize.height > 0 else {
            throw Failure("fixture has no surface")
        }
        view.frame = CGRect(origin: .zero, size: logicalSize)
        view.setNeedsLayout()
        view.layoutIfNeeded()
        let width = Int(logicalSize.width.rounded())
        let height = Int(logicalSize.height.rounded())
        var bytes = [UInt8](repeating: 0, count: width * height * 4)
        let colorSpace = CGColorSpaceCreateDeviceRGB()
        guard let context = CGContext(
            data: &bytes,
            width: width,
            height: height,
            bitsPerComponent: 8,
            bytesPerRow: width * 4,
            space: colorSpace,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ) else { throw Failure("create UIKit checkpoint context") }
        context.translateBy(x: 0, y: CGFloat(height))
        context.scaleBy(x: 1, y: -1)
        view.layer.render(in: context)
        return Pixels(width: width, height: height, bytes: bytes)
    }
}

private struct Pixels {
    let width: Int
    let height: Int
    let bytes: [UInt8]
}

private struct Failure: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}

private func operation(
    tag: UInt32,
    member: UInt32 = 0,
    payload: UnsafeRawPointer? = nil,
    count: Int = 0
) -> WhiskerMobileOperation {
    var value = WhiskerMobileOperation()
    value.tag = tag
    value.node = 1
    value.member = member
    value.payload = payload
    value.payload_count = count
    return value
}

private func boxPaint(_ command: [String: Any]) throws -> WhiskerMobileBoxPaint {
    var value = WhiskerMobileBoxPaint()
    value.background = try color(try object(command, "background"))
    guard let border = command["border"] as? [String: Any] else { return value }
    let widths = try numberArray(border, "widths").map(length)
    let colors = try objectArray(border, "colors").map(color)
    let styles = try stringArray(border, "styles").map(borderStyle)
    let radii = try numberArray(border, "radii").map(length)
    guard widths.count == 4, colors.count == 4, styles.count == 4, radii.count == 4 else {
        throw Failure("border arrays need four values")
    }
    value.widths = (widths[0], widths[1], widths[2], widths[3])
    value.colors = (colors[0], colors[1], colors[2], colors[3])
    value.styles = (styles[0], styles[1], styles[2], styles[3])
    value.radii = (radii[0], radii[1], radii[2], radii[3])
    return value
}

private func color(_ fixture: [String: Any]) throws -> WhiskerMobileColor {
    var value = WhiskerMobileColor()
    let rgba: (UInt8, UInt8, UInt8, Float)
    if try string(fixture, "kind") == "named" {
        switch try string(fixture, "value") {
        case "aqua": rgba = (0, 255, 255, 1)
        case "black": rgba = (0, 0, 0, 1)
        case "blue": rgba = (0, 0, 255, 1)
        case "green": rgba = (0, 128, 0, 1)
        case "transparent": rgba = (0, 0, 0, 0)
        case "white": rgba = (255, 255, 255, 1)
        default: throw Failure("unsupported fixture named color")
        }
    } else {
        rgba = (
            UInt8(try number(fixture, "red")),
            UInt8(try number(fixture, "green")),
            UInt8(try number(fixture, "blue")),
            Float(try number(fixture, "alpha"))
        )
    }
    value.kind = 1
    value.red = rgba.0
    value.green = rgba.1
    value.blue = rgba.2
    value.alpha = rgba.3
    return value
}

private func length(_ value: Double) -> WhiskerMobileLengthPercentage {
    WhiskerMobileLengthPercentage(length: Float(value), fraction: 0)
}

private func borderStyle(_ value: String) throws -> UInt32 {
    let names = ["none", "hidden", "solid", "dashed", "dotted", "double", "groove", "ridge", "inset", "outset"]
    guard let index = names.firstIndex(of: value) else { throw Failure("unknown border style") }
    return UInt32(index)
}

private var fixtureRoot: URL {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()
        .deletingLastPathComponent()
        .deletingLastPathComponent()
        .deletingLastPathComponent()
}

private func json(at url: URL) throws -> [String: Any] {
    try unwrap(try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any], url.path)
}

private func object(_ value: [String: Any], _ key: String) throws -> [String: Any] {
    try unwrap(value[key] as? [String: Any], key)
}

private func array(_ value: [String: Any], _ key: String) throws -> [[String: Any]] {
    try objectArray(value, key)
}

private func objectArray(_ value: [String: Any], _ key: String) throws -> [[String: Any]] {
    try unwrap(value[key] as? [[String: Any]], key)
}

private func numberArray(_ value: [String: Any], _ key: String) throws -> [Double] {
    try unwrap(value[key] as? [Double], key)
}

private func stringArray(_ value: [String: Any], _ key: String) throws -> [String] {
    try unwrap(value[key] as? [String], key)
}

private func string(_ value: [String: Any], _ key: String) throws -> String {
    try unwrap(value[key] as? String, key)
}

private func number(_ value: [String: Any], _ key: String) throws -> Double {
    try unwrap(value[key] as? NSNumber, key).doubleValue
}

private func unwrap<T>(_ value: T?, _ context: String) throws -> T {
    guard let value else { throw Failure("missing or invalid \(context)") }
    return value
}

private func largestDifference(_ left: [UInt8], _ right: [UInt8]) -> UInt8 {
    zip(left, right).map { $0 > $1 ? $0 - $1 : $1 - $0 }.max() ?? 0
}

private func assertPixelSamples(
    id: String,
    pixels: Pixels,
    samples: [[String: Any]]
) throws {
    for sample in samples {
        let point = try numberArray(sample, "point")
        guard point.count == 2 else { throw Failure("pixel sample needs two coordinates") }
        let x = Int(point[0].rounded(.down))
        let y = Int(point[1].rounded(.down))
        guard x >= 0, x < pixels.width, y >= 0, y < pixels.height else {
            throw Failure("pixel sample is outside the surface")
        }
        let offset = (y * pixels.width + x) * 4
        let actual = Array(pixels.bytes[offset..<(offset + 4)])
        let expectedColor = try color(try object(sample, "color"))
        let expected = [
            expectedColor.red,
            expectedColor.green,
            expectedColor.blue,
            UInt8((expectedColor.alpha * 255).rounded())
        ]
        let tolerance = UInt8((sample["tolerance"] as? NSNumber)?.uint8Value ?? 0)
        let difference = largestDifference(actual, expected)
        XCTAssertLessThanOrEqual(difference, tolerance, "\(id) sample (\(x), \(y))")
    }
}

private func assertPixelRelations(
    id: String,
    pixels: Pixels,
    relations: [[String: Any]]
) throws {
    for relation in relations {
        let firstPoint = try numberArray(relation, "first")
        let secondPoint = try numberArray(relation, "second")
        guard firstPoint.count == 2, secondPoint.count == 2 else {
            throw Failure("pixel relation needs two coordinates per point")
        }
        let first = try pixel(at: firstPoint, in: pixels)
        let second = try pixel(at: secondPoint, in: pixels)
        let firstLuminance = luminance(first)
        let secondLuminance = luminance(second)
        let minimum = UInt32(try number(relation, "minimum_difference"))
        let matches: Bool
        switch try string(relation, "relation") {
        case "lighter": matches = firstLuminance >= secondLuminance + minimum
        case "darker": matches = firstLuminance + minimum <= secondLuminance
        default: throw Failure("unknown pixel relation")
        }
        XCTAssertTrue(
            matches,
            "\(id) relation: \(first) (\(firstLuminance)) vs \(second) (\(secondLuminance))"
        )
    }
}

private func pixel(at point: [Double], in pixels: Pixels) throws -> [UInt8] {
    let x = Int(point[0].rounded(.down))
    let y = Int(point[1].rounded(.down))
    guard x >= 0, x < pixels.width, y >= 0, y < pixels.height else {
        throw Failure("pixel relation is outside the surface")
    }
    let offset = (y * pixels.width + x) * 4
    return Array(pixels.bytes[offset..<(offset + 4)])
}

private func luminance(_ color: [UInt8]) -> UInt32 {
    (UInt32(color[0]) * 299 + UInt32(color[1]) * 587 + UInt32(color[2]) * 114) / 1000
}
