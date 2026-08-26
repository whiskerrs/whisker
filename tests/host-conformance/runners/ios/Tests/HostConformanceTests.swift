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
                let type = try string($0, "type")
                return type == "present_box" || type == "present_scene"
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

    func testNoRepeatLeadingEdgeUsesHalfADevicePixel() {
        XCTAssertEqual(backgroundLeadingEdgeInset(deviceScale: 1), 0.5)
        XCTAssertEqual(backgroundLeadingEdgeInset(deviceScale: 2), 0.25)
        XCTAssertEqual(backgroundLeadingEdgeInset(deviceScale: 3), 1.0 / 6.0, accuracy: 0.000_001)
    }

    func testContentBoxRadiiUseTheCompleteInsetFromTheBorderBox() {
        XCTAssertEqual(
            insetCornerRadii(
                [CGSize(width: 30, height: 30), CGSize(width: 30, height: 30),
                 CGSize(width: 30, height: 30), CGSize(width: 30, height: 30)],
                top: 10,
                right: 20,
                bottom: 20,
                left: 20
            ),
            [
                CGSize(width: 10, height: 20),
                CGSize(width: 10, height: 20),
                CGSize(width: 10, height: 10),
                CGSize(width: 10, height: 10)
            ]
        )
    }

    func testBackgroundLayerArrayRejectsAnUnregisteredResourceTransactionally() throws {
        var pixel: [UInt8] = [0, 0, 255, 255]
        let context = try XCTUnwrap(CGContext(
            data: &pixel,
            width: 1,
            height: 1,
            bitsPerComponent: 8,
            bytesPerRow: 4,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ))
        let raster = try XCTUnwrap(context.makeImage())
        let view = WhiskerView(frame: .zero)
        let registration = WhiskerElementRegistration(
            elementType: 1,
            name: WhiskerBuiltInElements.viewName,
            childPolicy: .elements,
            measurement: .none
        )
        XCTAssertTrue(WhiskerElementRegistry.bind([registration]))
        var stops = [WhiskerMobileGradientStop(), WhiskerMobileGradientStop()]
        for index in stops.indices {
            stops[index].color.kind = 1
            stops[index].color.red = 255
            stops[index].color.alpha = 1
            stops[index].position.fraction = Float(index)
        }
        stops.withUnsafeMutableBufferPointer { stopBuffer in
            var valid = WhiskerMobileBackgroundLayer()
            valid.image.kind = UInt32(WHISKER_BACKGROUND_LINEAR)
            valid.image.payload = UnsafeRawPointer(stopBuffer.baseAddress!)
            valid.image.payload_count = stopBuffer.count
            valid.size_kind = UInt32(WHISKER_BACKGROUND_SIZE_EXPLICIT)
            valid.size_width.length = 10
            valid.size_height.length = 10
            valid.repeat_x = UInt32(WHISKER_BACKGROUND_NO_REPEAT)
            valid.repeat_y = UInt32(WHISKER_BACKGROUND_NO_REPEAT)
            valid.origin = UInt32(WHISKER_BACKGROUND_BOX_BORDER)
            valid.clip = UInt32(WHISKER_BACKGROUND_BOX_BORDER)
            let resourceID = UInt64.max
            var resourcePayload = resourceID
            withUnsafePointer(to: &resourcePayload) { resourcePointer in
                var invalid = valid
                invalid.image.kind = UInt32(WHISKER_BACKGROUND_RESOURCE)
                invalid.image.payload = UnsafeRawPointer(resourcePointer)
                invalid.image.payload_count = 1
                var layers = [valid, invalid]
                layers.withUnsafeMutableBufferPointer { layerBuffer in
                    var operations = [
                        operation(tag: UInt32(WHISKER_OP_CREATE), node: 1, member: 1),
                        operation(
                            tag: UInt32(WHISKER_OP_BACKGROUND_LAYERS),
                            node: 1,
                            payload: UnsafeRawPointer(layerBuffer.baseAddress!),
                            count: layerBuffer.count
                        )
                    ]
                    operations.withUnsafeMutableBufferPointer { operationBuffer in
                        var frame = WhiskerMobileFrame()
                        frame.abi_major = UInt16(WHISKER_MOBILE_ABI_MAJOR)
                        frame.abi_minor = UInt16(WHISKER_MOBILE_ABI_MINOR)
                        frame.protocol_major = 1
                        frame.mode = UInt8(WHISKER_FRAME_SNAPSHOT)
                        frame.surface = 1
                        frame.scene_epoch = 1
                        frame.viewport_epoch = 1
                        frame.frame_id = 1
                        frame.target_revision = 1
                        frame.operations = UnsafePointer(operationBuffer.baseAddress!)
                        frame.operation_count = operationBuffer.count
                        var response = WhiskerMobileApplyResponse()
                        XCTAssertTrue(view.applyConformanceFrame(frame, response: &response))
                        XCTAssertEqual(response.status, UInt8(WHISKER_APPLY_REJECTED))
                        XCTAssertTrue(
                            view.registerRasterResource(id: resourceID, image: raster)
                        )
                        XCTAssertTrue(view.applyConformanceFrame(frame, response: &response))
                        XCTAssertEqual(response.status, UInt8(WHISKER_APPLY_ACCEPTED))
                    }
                }
            }
        }
    }

    func testBackgroundTileOriginsRespectEachRepeatAxis() {
        XCTAssertEqual(
            backgroundTileOrigins(
                base: 20,
                tileSize: 20,
                positioning: 0..<100,
                coverage: 0..<100,
                repeatMode: .repeat
            ),
            [0, 20, 40, 60, 80]
        )
        XCTAssertEqual(
            backgroundTileOrigins(
                base: 20,
                tileSize: 20,
                positioning: 0..<100,
                coverage: 0..<100,
                repeatMode: .noRepeat
            ),
            [20]
        )
        XCTAssertEqual(
            backgroundTileOrigins(
                base: 7,
                tileSize: 30,
                positioning: 0..<100,
                coverage: 0..<100,
                repeatMode: .space
            ),
            [0, 35, 70]
        )
        XCTAssertEqual(
            backgroundTileOrigins(
                base: 5,
                tileSize: 32,
                positioning: 0..<48,
                coverage: 0..<48,
                repeatMode: .space
            ),
            [5]
        )
        XCTAssertEqual(
            backgroundTileOrigins(
                base: 5,
                tileSize: 36,
                positioning: 0..<72,
                coverage: 0..<72,
                repeatMode: .round
            ),
            [-31, 5, 41]
        )
        XCTAssertEqual(
            backgroundRoundTileSize(
                originalTileSize: 32,
                positioningSize: 72,
                repeatMode: .round
            ),
            36
        )
        XCTAssertEqual(
            backgroundRoundTileSize(
                originalTileSize: 32,
                positioningSize: 72,
                repeatMode: .repeat
            ),
            32
        )

        let mixed = HostBackgroundGeometry(
            positionX: WhiskerMobileLengthPercentage(),
            positionY: WhiskerMobileLengthPercentage(length: 7, fraction: 0),
            sizeWidth: WhiskerMobileLengthPercentage(length: 30, fraction: 0),
            sizeHeight: WhiskerMobileLengthPercentage(length: 20, fraction: 0),
            repeatX: .space,
            repeatY: .noRepeat,
            origin: .padding,
            clip: .border
        )
        XCTAssertEqual(
            mixed.tileRects(
                in: CGRect(x: 0, y: 0, width: 100, height: 60),
                covering: CGRect(x: 0, y: 0, width: 100, height: 60)
            ),
            [
                CGRect(x: 0, y: 7, width: 30, height: 20),
                CGRect(x: 35, y: 7, width: 30, height: 20),
                CGRect(x: 70, y: 7, width: 30, height: 20)
            ]
        )

        let roundMixed = HostBackgroundGeometry(
            positionX: WhiskerMobileLengthPercentage(),
            positionY: WhiskerMobileLengthPercentage(length: 7, fraction: 0),
            sizeWidth: WhiskerMobileLengthPercentage(length: 32, fraction: 0),
            sizeHeight: WhiskerMobileLengthPercentage(length: 20, fraction: 0),
            repeatX: .round,
            repeatY: .noRepeat,
            origin: .padding,
            clip: .border
        )
        XCTAssertEqual(
            roundMixed.tileRects(
                in: CGRect(x: 0, y: 0, width: 72, height: 60),
                covering: CGRect(x: 0, y: 0, width: 72, height: 60)
            ),
            [
                CGRect(x: 0, y: 7, width: 36, height: 20),
                CGRect(x: 36, y: 7, width: 36, height: 20)
            ]
        )
    }

    func testResourceServiceIgnoresStaleCompletionAndOldRelease() throws {
        let store = HostResourceStore()
        var completions = [(Result<Data, Error>) -> Void]()
        let service = HostResourceService(store: store, urlLoader: { _, completion in
            completions.append(completion)
            return {}
        })

        XCTAssertTrue(service.load(
            id: 7,
            generation: 1,
            source: .url("https://example.com/old.png")
        ))
        XCTAssertTrue(service.load(
            id: 7,
            generation: 2,
            source: .url("https://example.com/new.png")
        ))
        XCTAssertEqual(completions.count, 2)

        completions[1](.success(try fixturePNGData()))
        XCTAssertEqual(
            try awaitResourceState(in: service, id: 7, generation: 2),
            .ready(width: 2, height: 2)
        )
        completions[0](.failure(URLError(.cancelled)))
        RunLoop.current.run(until: Date().addingTimeInterval(0.02))
        XCTAssertEqual(service.state(id: 7, generation: 2), .ready(width: 2, height: 2))

        XCTAssertTrue(service.release(id: 7, generation: 1))
        XCTAssertNotNil(store.rasterImage(id: 7))
        XCTAssertEqual(service.state(id: 7, generation: 1), .released)
        XCTAssertTrue(service.release(id: 7, generation: 2))
        XCTAssertNil(store.rasterImage(id: 7))
    }

    func testResourceServiceReportsDecodeFailure() throws {
        let service = HostResourceService(store: HostResourceStore())
        XCTAssertTrue(service.load(
            id: 8,
            generation: 1,
            source: .bytes(mediaType: "image/png", data: Data([0, 1, 2, 3]))
        ))
        XCTAssertEqual(
            try awaitResourceState(in: service, id: 8, generation: 1),
            .failed
        )
    }

    func testResourceChannelCopiesBorrowedBytesAndEncodesTypedEvents() throws {
        let view = WhiskerView(frame: .zero)
        var bytes = try fixturePNGData()
        let mediaType = Array("image/png".utf8CString)
        let accepted = mediaType.withUnsafeBufferPointer { mediaBuffer in
            bytes.withUnsafeBytes { dataBuffer in
                var command = WhiskerMobileResourceCommand()
                command.command = UInt32(WHISKER_RESOURCE_COMMAND_LOAD)
                command.kind = UInt32(WHISKER_RESOURCE_RASTER_IMAGE)
                command.source = UInt32(WHISKER_RESOURCE_SOURCE_BYTES)
                command.resource = 19
                command.generation = 1
                command.identifier = WhiskerStringRef(
                    ptr: mediaBuffer.baseAddress,
                    len: mediaBuffer.count - 1
                )
                command.data = WhiskerBytesRef(
                    ptr: dataBuffer.baseAddress?.assumingMemoryBound(to: UInt8.self),
                    len: dataBuffer.count
                )
                return withUnsafePointer(to: &command) { pointer in
                    whiskerIOSResourceCommand(
                        Unmanaged.passUnretained(view).toOpaque(),
                        pointer
                    )
                }
            }
        }
        XCTAssertTrue(accepted)
        bytes.resetBytes(in: bytes.startIndex..<bytes.endIndex)
        XCTAssertEqual(
            try awaitResourceState(in: view, id: 19, generation: 1),
            .ready(width: 2, height: 2)
        )

        let failed = WhiskerRasterResourceEvent(
            id: 19,
            generation: 2,
            state: .failed,
            failureCode: .decode,
            diagnostic: "invalid png"
        )
        let encoded = withMobileResourceEvent(failed) { raw -> Bool in
            XCTAssertEqual(raw.status, UInt32(WHISKER_RESOURCE_EVENT_FAILED))
            XCTAssertEqual(raw.failure_code, UInt32(WHISKER_RESOURCE_FAILURE_DECODE))
            XCTAssertEqual(raw.resource, 19)
            XCTAssertEqual(raw.generation, 2)
            XCTAssertEqual(hostString(raw.diagnostic), "invalid png")
            return true
        }
        XCTAssertEqual(encoded, true)
    }
}

@MainActor
private final class Driver {
    private let id: String
    private let view = WhiskerView(frame: .zero)
    private var logicalSize = CGSize.zero
    private var surfaceScale: CGFloat = 1
    private var checkpoint: Pixels?

    init(id: String) throws {
        self.id = id
        let registration = WhiskerElementRegistration(
            elementType: 1,
            name: WhiskerBuiltInElements.viewName,
            childPolicy: .elements,
            measurement: .none
        )
        let textRegistration = WhiskerElementRegistration(
            elementType: 2,
            name: WhiskerBuiltInElements.textName,
            childPolicy: .plainText,
            measurement: .text
        )
        guard WhiskerElementRegistry.bind([registration, textRegistration]) else {
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
                surfaceScale = CGFloat(try number(command, "scale"))
            case "register_raster_resource":
                try registerRasterResource(command)
            case "load_raster_resource":
                try loadRasterResource(command)
            case "release_raster_resource":
                try releaseRasterResource(command)
            case "checkpoint_resource":
                try checkpointRasterResource(command)
            case "present_box":
                try present(command)
            case "present_scene":
                try presentScene(command)
            case "checkpoint":
                let name = try string(command, "name")
                guard name == "paint.box" ||
                    name == "paint.background-layers.linear-gradient" ||
                    name == "paint.background-layers.radial-gradient" ||
                    name == "paint.background-layers.conic-gradient" ||
                    name == "paint.background-layers.explicit-size-no-repeat" ||
                    name == "paint.background-layers.position-length-percentage" ||
                    name == "paint.background-layers.origin-border-box" ||
                    name == "paint.background-layers.clip-padding-box" ||
                    name == "paint.background-layers.repeat-x" ||
                    name == "paint.background-layers.repeat-y" ||
                    name == "paint.background-layers.repeat-space" ||
                    name == "paint.background-layers.repeat-space-single" ||
                    name == "paint.background-layers.repeat-round-x" ||
                    name == "paint.background-layers.repeat-round-y" ||
                    name == "paint.background-layers.repeat-round-position" ||
                    name == "paint.background-layers.origin-content-box" ||
                    name == "paint.background-layers.clip-content-box" ||
                    name == "paint.background-layers.clip-border-area" ||
                    name == "paint.background-layers.stacking" ||
                    name == "paint.background-layers.resource-image" ||
                    name == "paint.background-layers.resource-lifecycle" ||
                    name == "paint.background-layers.intrinsic-auto" ||
                    name == "paint.background-layers.size-cover" ||
                    name == "paint.background-layers.size-contain" ||
                    name == "paint.background-layers.round-auto-aspect-ratio" ||
                    name == "paint.visual-effects.box-shadow-offset" ||
                    name == "paint.visual-effects.box-shadow-spread" ||
                    name == "paint.visual-effects.box-shadow-blur" ||
                    name == "paint.visual-effects.box-shadow-inset" ||
                    name == "paint.visual-effects.box-shadow-multiple" ||
                    name == "paint.visual-effects.clip-path-inset" ||
                    name == "paint.visual-effects.clip-path-circle" ||
                    name == "paint.visual-effects.clip-path-ellipse" ||
                    name == "paint.visual-effects.clip-path-path-nonzero" ||
                    name == "paint.visual-effects.clip-path-path-evenodd" ||
                    name == "paint.visual-effects.backdrop-blur" ||
                    name == "paint.visual-effects.image-rendering-pixelated" ||
                    name == "paint.transform.projective-plane" ||
                    name == "paint.transform.motion-path-line" ||
                    name == "paint.transform.motion-path-curves" ||
                    name == "paint.transform.motion-path-ellipses" ||
                    name == "paint.transform.motion-path-inset" ||
                    name == "paint.transform.motion-path-arcs" ||
                    name == "paint.text.shadow-single" ||
                    name == "paint.text.decoration-lynx" ||
                    name == "paint.text.align-lynx" ||
                    name == "paint.text.indent-lynx" else {
                    throw Failure("unsupported UIKit checkpoint")
                }
                if name == "paint.visual-effects.backdrop-blur" {
                    XCTAssertTrue(
                        containsActiveBackdropBlur(view),
                        "\(id) must project backdrop blur to UIVisualEffectView"
                    )
                }
                if name == "paint.transform.projective-plane" {
                    XCTAssertTrue(
                        containsProjectiveTransform(view),
                        "\(id) must project the 4x4 matrix to CATransform3D"
                    )
                }
                if name == "paint.text.shadow-single" {
                    let label = try XCTUnwrap(findLabel(view))
                    XCTAssertEqual(label.attributedText?.string ?? label.text, "Whisker")
                    let shadow = try XCTUnwrap(
                        label.attributedText?.attribute(.shadow, at: 0, effectiveRange: nil)
                            as? NSShadow
                    )
                    XCTAssertEqual(shadow.shadowOffset, CGSize(width: 3, height: 4))
                    XCTAssertEqual(shadow.shadowBlurRadius, 2)
                }
                if name == "paint.text.decoration-lynx" {
                    let labels = findTextLabels(view)
                    XCTAssertEqual(labels.count, 5)
                    XCTAssertEqual(labels[0].whiskerDecoration?.style, .solid)
                    XCTAssertEqual(labels[1].whiskerDecoration?.style, .double)
                    XCTAssertEqual(labels[2].whiskerDecoration?.style, .dotted)
                    XCTAssertEqual(labels[3].whiskerDecoration?.style, .dashed)
                    XCTAssertEqual(labels[4].whiskerDecoration?.style, .wavy)
                    XCTAssertEqual(labels[0].whiskerDecoration?.line, .underline)
                    XCTAssertEqual(labels[4].whiskerDecoration?.line, .lineThrough)
                }
                if name == "paint.text.align-lynx" {
                    let labels = findTextLabels(view)
                    XCTAssertEqual(labels.map(\.textAlignment), [.left, .right, .center, .left, .right])
                }
                if name == "paint.text.indent-lynx" {
                    let labels = findTextLabels(view)
                    XCTAssertEqual(labels.count, 2)
                    let indents = labels.map { label -> CGFloat in
                        let paragraph = label.attributedText?.attribute(
                            .paragraphStyle,
                            at: 0,
                            effectiveRange: nil
                        ) as? NSParagraphStyle
                        return paragraph?.firstLineHeadIndent ?? 0
                    }
                    XCTAssertEqual(indents, [24, 30])
                }
                let pixels = try capture()
                checkpoint = pixels
                if name != "paint.transform.projective-plane",
                   let samples = command["samples"] as? [[String: Any]] {
                    try assertPixelSamples(id: id, pixels: pixels, samples: samples)
                }
                if name != "paint.visual-effects.backdrop-blur",
                   let relations = command["relations"] as? [[String: Any]] {
                    try assertPixelRelations(id: id, pixels: pixels, relations: relations)
                }
            default:
                throw Failure("unsupported UIKit paint command")
            }
        }
        return try unwrap(checkpoint, "paint checkpoint")
    }

    private func registerRasterResource(_ command: [String: Any]) throws {
        let id = UInt64(try number(command, "id"))
        let width = Int(try number(command, "width"))
        let height = Int(try number(command, "height"))
        let pixels = try objectArray(command, "pixels").map { try color($0) }
        guard id != 0, width > 0, height > 0, pixels.count == width * height else {
            throw Failure("invalid raster resource")
        }
        var bytes = pixels.flatMap { pixel -> [UInt8] in
            let alpha = CGFloat(pixel.alpha)
            return [
                UInt8((CGFloat(pixel.red) * alpha).rounded()),
                UInt8((CGFloat(pixel.green) * alpha).rounded()),
                UInt8((CGFloat(pixel.blue) * alpha).rounded()),
                UInt8((alpha * 255).rounded())
            ]
        }
        let colorSpace = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
        guard let context = CGContext(
            data: &bytes,
            width: width,
            height: height,
            bitsPerComponent: 8,
            bytesPerRow: width * 4,
            space: colorSpace,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ), let image = context.makeImage(), view.registerRasterResource(id: id, image: image) else {
            throw Failure("register raster resource")
        }
    }

    private func loadRasterResource(_ command: [String: Any]) throws {
        let id = UInt64(try number(command, "id"))
        let generation = UInt64(try number(command, "generation"))
        let fixture = try object(command, "source")
        let sourceKind: UInt32
        let identifier: String
        let data: Data
        switch try string(fixture, "kind") {
        case "bytes":
            guard let decoded = Data(base64Encoded: try string(fixture, "base64")) else {
                throw Failure("invalid base64 raster resource")
            }
            sourceKind = UInt32(WHISKER_RESOURCE_SOURCE_BYTES)
            identifier = try string(fixture, "media_type")
            data = decoded
        case "url":
            sourceKind = UInt32(WHISKER_RESOURCE_SOURCE_URL)
            identifier = try string(fixture, "value")
            data = Data()
        default:
            throw Failure("unsupported raster resource source")
        }
        guard dispatchResourceLoad(
            to: view,
            id: id,
            generation: generation,
            source: sourceKind,
            identifier: identifier,
            data: data
        ) else {
            throw Failure("load raster resource")
        }
    }

    private func releaseRasterResource(_ command: [String: Any]) throws {
        var raw = WhiskerMobileResourceCommand()
        raw.command = UInt32(WHISKER_RESOURCE_COMMAND_RELEASE)
        raw.source = UInt32(WHISKER_RESOURCE_SOURCE_NONE)
        raw.resource = UInt64(try number(command, "id"))
        raw.generation = UInt64(try number(command, "generation"))
        let accepted = withUnsafePointer(to: &raw) { pointer in
            whiskerIOSResourceCommand(Unmanaged.passUnretained(view).toOpaque(), pointer)
        }
        guard accepted else {
            throw Failure("release raster resource")
        }
    }

    private func checkpointRasterResource(_ command: [String: Any]) throws {
        let id = UInt64(try number(command, "id"))
        let generation = UInt64(try number(command, "generation"))
        let expected = try string(command, "state")
        let state = try awaitResourceState(in: view, id: id, generation: generation)
        switch (expected, state) {
        case let ("ready", .ready(width, height)):
            guard width == Int(try number(command, "width")),
                  height == Int(try number(command, "height")) else {
                throw Failure("raster resource dimensions disagree")
            }
        case ("failed", .failed), ("released", .released):
            break
        default:
            throw Failure("raster resource state disagrees")
        }
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
        let content = try command["content_box"].map { _ in
            try numberArray(command, "content_box")
        } ?? [0, 0, values[2], values[3]]
        guard content.count == 4 else { throw Failure("content box needs four values") }
        layout.content = WhiskerMobileRect(
            x: Float(content[0]),
            y: Float(content[1]),
            width: Float(content[2]),
            height: Float(content[3])
        )
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

    private func presentScene(_ command: [String: Any]) throws {
        let revision = UInt64(try number(command, "revision"))
        let fixtures = try objectArray(command, "nodes").map(sceneNode)
        let textPayloads = UnsafeMutablePointer<WhiskerMobileText>.allocate(
            capacity: max(fixtures.count, 1)
        )
        var textStrings = [UnsafeMutablePointer<CChar>?](repeating: nil, count: fixtures.count)
        for (index, fixture) in fixtures.enumerated() {
            var payload = WhiskerMobileText()
            if let text = fixture.text {
                let bytes = Array(text.value.utf8CString)
                let storage = UnsafeMutablePointer<CChar>.allocate(capacity: bytes.count)
                storage.initialize(from: bytes, count: bytes.count)
                textStrings[index] = storage
                payload.text = WhiskerStringRef(ptr: UnsafePointer(storage), len: bytes.count - 1)
                payload.font_size = text.fontSize
                payload.font_weight = text.fontWeight
                payload.color = text.color
                if let offset = text.shadowOffset {
                    payload.shadow_flags = 1
                    payload.shadow_offset_x = Float(offset.width)
                    payload.shadow_offset_y = Float(offset.height)
                    payload.shadow_blur_radius = text.shadowBlurRadius
                    payload.shadow_color = text.shadowColor
                }
                payload.decoration_flags = text.decorationFlags
                payload.decoration_style = text.decorationStyle
                payload.decoration_color = text.decorationColor
                payload.alignment = text.alignment
                payload.indent_logical_pixels = text.indentLogicalPixels
                payload.indent_percentage = text.indentPercentage
            }
            textPayloads.advanced(by: index).initialize(to: payload)
        }
        defer {
            textPayloads.deinitialize(count: fixtures.count)
            textPayloads.deallocate()
            for storage in textStrings {
                storage?.deallocate()
            }
        }
        var layouts = fixtures.map(\.layout)
        var paints = fixtures.map(\.paint)
        var transforms = fixtures.flatMap { fixture in
            fixture.transform ?? [Float](repeating: 0, count: 16)
        }
        let layersByFixture = fixtures.map(\.resolvedBackgroundLayers)
        var layerRanges = [Range<Int>]()
        var stagedLayers = [ScenePaintLayer]()
        for layers in layersByFixture {
            let start = stagedLayers.count
            stagedLayers.append(contentsOf: layers)
            layerRanges.append(start..<stagedLayers.count)
        }
        var shadowRanges = [Range<Int>]()
        var stagedShadows = [WhiskerMobileBoxShadow]()
        for fixture in fixtures {
            let start = stagedShadows.count
            stagedShadows.append(contentsOf: fixture.boxShadows)
            shadowRanges.append(start..<stagedShadows.count)
        }
        var gradientStops = [WhiskerMobileGradientStop]()
        var gradientOffsets = [Int?]()
        for layer in stagedLayers {
            switch layer.image {
            case .resource:
                gradientOffsets.append(nil)
            case let .linear(gradient):
                gradientOffsets.append(gradientStops.count)
                gradientStops.append(contentsOf: gradient.stops)
            case let .radial(gradient):
                gradientOffsets.append(gradientStops.count)
                gradientStops.append(contentsOf: gradient.stops)
            case let .conic(gradient):
                gradientOffsets.append(gradientStops.count)
                gradientStops.append(contentsOf: gradient.stops)
            }
        }
        var resourceIDs = stagedLayers.map { layer -> UInt64 in
            if case let .resource(id) = layer.image { return id }
            return 0
        }
        var radialPayloads = [WhiskerMobileRadialGradient](
            repeating: WhiskerMobileRadialGradient(),
            count: stagedLayers.count
        )
        var conicPayloads = [WhiskerMobileConicGradient](
            repeating: WhiskerMobileConicGradient(),
            count: stagedLayers.count
        )
        var backgroundPayloads = [WhiskerMobileBackgroundLayer](
            repeating: WhiskerMobileBackgroundLayer(),
            count: stagedLayers.count
        )
        let clipStorageCount = max(fixtures.count, 1)
        let clipInsets = UnsafeMutablePointer<WhiskerMobileClipInset>.allocate(
            capacity: clipStorageCount
        )
        let clipPaths = UnsafeMutablePointer<WhiskerMobileClipPath>.allocate(
            capacity: clipStorageCount
        )
        let clipCircles = UnsafeMutablePointer<WhiskerMobileClipCircle>.allocate(capacity: clipStorageCount)
        let clipEllipses = UnsafeMutablePointer<WhiskerMobileClipEllipse>.allocate(capacity: clipStorageCount)
        let clipPathPayloads = UnsafeMutablePointer<WhiskerMobileClipPathCommands>.allocate(
            capacity: clipStorageCount
        )
        var clipCommandBuffers = [UnsafeMutablePointer<WhiskerMobilePathCommand>?](
            repeating: nil, count: fixtures.count
        )
        for (index, fixture) in fixtures.enumerated() {
            clipInsets.advanced(by: index).initialize(
                to: fixture.clipPath?.inset ?? WhiskerMobileClipInset()
            )
            clipCircles.advanced(by: index).initialize(to: fixture.clipPath?.circle ?? WhiskerMobileClipCircle())
            clipEllipses.advanced(by: index).initialize(to: fixture.clipPath?.ellipse ?? WhiskerMobileClipEllipse())
            var pathPayload = WhiskerMobileClipPathCommands()
            if let clip = fixture.clipPath, !clip.pathCommands.isEmpty {
                let commands = UnsafeMutablePointer<WhiskerMobilePathCommand>.allocate(
                    capacity: clip.pathCommands.count
                )
                for (commandIndex, command) in clip.pathCommands.enumerated() {
                    commands.advanced(by: commandIndex).initialize(to: command)
                }
                clipCommandBuffers[index] = commands
                pathPayload.fill_rule = clip.pathFillRule
                pathPayload.commands = UnsafePointer(commands)
                pathPayload.command_count = clip.pathCommands.count
            }
            clipPathPayloads.advanced(by: index).initialize(to: pathPayload)
            var path = WhiskerMobileClipPath()
            if let clip = fixture.clipPath {
                path.reference_box = clip.referenceBox
                path.shape_kind = clip.shapeKind
                path.payload = switch clip.shapeKind {
                case UInt32(WHISKER_CLIP_SHAPE_CIRCLE): UnsafeRawPointer(clipCircles.advanced(by: index))
                case UInt32(WHISKER_CLIP_SHAPE_ELLIPSE): UnsafeRawPointer(clipEllipses.advanced(by: index))
                case UInt32(WHISKER_CLIP_SHAPE_PATH): UnsafeRawPointer(clipPathPayloads.advanced(by: index))
                default: UnsafeRawPointer(clipInsets.advanced(by: index))
                }
                path.payload_count = 1
            }
            clipPaths.advanced(by: index).initialize(to: path)
        }
        defer {
            clipInsets.deinitialize(count: fixtures.count)
            clipInsets.deallocate()
            clipPaths.deinitialize(count: fixtures.count)
            clipPaths.deallocate()
            clipCircles.deinitialize(count: fixtures.count)
            clipCircles.deallocate()
            clipEllipses.deinitialize(count: fixtures.count)
            clipEllipses.deallocate()
            clipPathPayloads.deinitialize(count: fixtures.count)
            clipPathPayloads.deallocate()
            for (index, commands) in clipCommandBuffers.enumerated() {
                commands?.deinitialize(count: fixtures[index].clipPath?.pathCommands.count ?? 0)
                commands?.deallocate()
            }
        }
        try layouts.withUnsafeMutableBufferPointer { layoutBuffer in
            try paints.withUnsafeMutableBufferPointer { paintBuffer in
                try transforms.withUnsafeMutableBufferPointer { transformBuffer in
                    try gradientStops.withUnsafeMutableBufferPointer { gradientBuffer in
                        for (index, layer) in stagedLayers.enumerated() {
                            guard let offset = gradientOffsets[index] else { continue }
                            let stops = UnsafePointer(
                                gradientBuffer.baseAddress!.advanced(by: offset)
                            )
                            if case let .radial(radial) = layer.image {
                                radialPayloads[index].center_x = WhiskerMobileLengthPercentage(
                                    length: radial.center[0], fraction: 0
                                )
                                radialPayloads[index].center_y = WhiskerMobileLengthPercentage(
                                    length: radial.center[1], fraction: 0
                                )
                                radialPayloads[index].radius_x = WhiskerMobileLengthPercentage(
                                    length: radial.radii[0], fraction: 0
                                )
                                radialPayloads[index].radius_y = WhiskerMobileLengthPercentage(
                                    length: radial.radii[1], fraction: 0
                                )
                                radialPayloads[index].stops = stops
                                radialPayloads[index].stop_count = radial.stops.count
                            } else if case let .conic(conic) = layer.image {
                                conicPayloads[index].center_x = WhiskerMobileLengthPercentage(
                                    length: conic.center[0], fraction: 0
                                )
                                conicPayloads[index].center_y = WhiskerMobileLengthPercentage(
                                    length: conic.center[1], fraction: 0
                                )
                                conicPayloads[index].stops = stops
                                conicPayloads[index].stop_count = conic.stops.count
                            }
                        }
                        try radialPayloads.withUnsafeMutableBufferPointer { radialBuffer in
                            try conicPayloads.withUnsafeMutableBufferPointer { conicBuffer in
                            try resourceIDs.withUnsafeMutableBufferPointer { resourceBuffer in
                            for (index, layer) in stagedLayers.enumerated() {
                                let offset = gradientOffsets[index]
                                let geometry = layer.geometry
                                backgroundPayloads[index].position_x = geometry.position[0]
                                backgroundPayloads[index].position_y = geometry.position[1]
                                switch geometry.size {
                                case .auto:
                                    backgroundPayloads[index].size_kind = UInt32(
                                        WHISKER_BACKGROUND_SIZE_AUTO
                                    )
                                case let .explicit(width, height):
                                    backgroundPayloads[index].size_kind = UInt32(
                                        WHISKER_BACKGROUND_SIZE_EXPLICIT
                                    )
                                    backgroundPayloads[index].size_width = width
                                    backgroundPayloads[index].size_height = height
                                case .cover:
                                    backgroundPayloads[index].size_kind = UInt32(
                                        WHISKER_BACKGROUND_SIZE_COVER
                                    )
                                case .contain:
                                    backgroundPayloads[index].size_kind = UInt32(
                                        WHISKER_BACKGROUND_SIZE_CONTAIN
                                    )
                                case let .width(width):
                                    backgroundPayloads[index].size_kind = UInt32(
                                        WHISKER_BACKGROUND_SIZE_WIDTH
                                    )
                                    backgroundPayloads[index].size_width = width
                                case let .height(height):
                                    backgroundPayloads[index].size_kind = UInt32(
                                        WHISKER_BACKGROUND_SIZE_HEIGHT
                                    )
                                    backgroundPayloads[index].size_height = height
                                }
                                backgroundPayloads[index].repeat_x = geometry.repeatX
                                backgroundPayloads[index].repeat_y = geometry.repeatY
                                backgroundPayloads[index].origin = geometry.origin
                                backgroundPayloads[index].clip = geometry.clip
                                backgroundPayloads[index].attachment = UInt32(
                                    WHISKER_BACKGROUND_ATTACHMENT_SCROLL
                                )
                                backgroundPayloads[index].blend_mode = UInt32(
                                    WHISKER_BACKGROUND_BLEND_NORMAL
                                )
                                switch layer.image {
                                case .resource:
                                    backgroundPayloads[index].image.kind = UInt32(
                                        WHISKER_BACKGROUND_RESOURCE
                                    )
                                    backgroundPayloads[index].image.payload = UnsafeRawPointer(
                                        resourceBuffer.baseAddress!.advanced(by: index)
                                    )
                                    backgroundPayloads[index].image.payload_count = 1
                                case let .linear(gradient):
                                    guard let offset else { throw Failure("missing gradient stops") }
                                    backgroundPayloads[index].image.kind = UInt32(
                                        WHISKER_BACKGROUND_LINEAR
                                    )
                                    backgroundPayloads[index].image.scalar = gradient.angleDegrees
                                    backgroundPayloads[index].image.payload = UnsafeRawPointer(
                                        gradientBuffer.baseAddress!.advanced(by: offset)
                                    )
                                    backgroundPayloads[index].image.payload_count = gradient.stops.count
                                case .radial:
                                    backgroundPayloads[index].image.kind = UInt32(
                                        WHISKER_BACKGROUND_RADIAL
                                    )
                                    backgroundPayloads[index].image.payload = UnsafeRawPointer(
                                        radialBuffer.baseAddress!.advanced(by: index)
                                    )
                                    backgroundPayloads[index].image.payload_count = 1
                                case let .conic(gradient):
                                    backgroundPayloads[index].image.kind = UInt32(
                                        WHISKER_BACKGROUND_CONIC
                                    )
                                    backgroundPayloads[index].image.scalar = gradient.fromDegrees
                                    backgroundPayloads[index].image.payload = UnsafeRawPointer(
                                        conicBuffer.baseAddress!.advanced(by: index)
                                    )
                                    backgroundPayloads[index].image.payload_count = 1
                                }
                            }
                            try backgroundPayloads.withUnsafeMutableBufferPointer {
                                backgroundBuffer in
                            try stagedShadows.withUnsafeMutableBufferPointer { shadowBuffer in
                            var operations = fixtures.map {
                                operation(
                                    tag: UInt32(WHISKER_OP_CREATE),
                                    node: $0.id,
                                    member: $0.text == nil ? 1 : 2
                                )
                            }
                            var childCounts: [UInt64: UInt32] = [:]
                            for fixture in fixtures {
                                guard let parent = fixture.parent else { continue }
                                let index = childCounts[parent, default: 0]
                                operations.append(operation(
                                    tag: UInt32(WHISKER_OP_INSERT),
                                    parent: parent,
                                    child: fixture.id,
                                    index: index
                                ))
                                childCounts[parent] = index + 1
                            }
                            for (index, fixture) in fixtures.enumerated() {
                                operations.append(operation(
                                    tag: UInt32(WHISKER_OP_LAYOUT),
                                    node: fixture.id,
                                    payload: UnsafeRawPointer(
                                        layoutBuffer.baseAddress!.advanced(by: index)
                                    ),
                                    count: 1
                                ))
                                operations.append(operation(
                                    tag: UInt32(WHISKER_OP_PAINT),
                                    node: fixture.id,
                                    payload: UnsafeRawPointer(
                                        paintBuffer.baseAddress!.advanced(by: index)
                                    ),
                                    count: 1
                                ))
                                if fixture.text != nil {
                                    operations.append(operation(
                                        tag: UInt32(WHISKER_OP_TEXT),
                                        node: fixture.id,
                                        payload: UnsafeRawPointer(textPayloads.advanced(by: index)),
                                        count: 1
                                    ))
                                }
                                operations.append(operation(
                                    tag: UInt32(WHISKER_OP_CLIP),
                                    node: fixture.id,
                                    flags: fixture.clipFlags
                                ))
                                if fixture.transform != nil {
                                    operations.append(operation(
                                        tag: UInt32(WHISKER_OP_TRANSFORM),
                                        node: fixture.id,
                                        payload: UnsafeRawPointer(
                                            transformBuffer.baseAddress!.advanced(by: index * 16)
                                        ),
                                        count: 16
                                    ))
                                }
                                if let opacity = fixture.opacity {
                                    operations.append(operation(
                                        tag: UInt32(WHISKER_OP_OPACITY),
                                        node: fixture.id,
                                        scalar: opacity
                                    ))
                                }
                                if let visible = fixture.visible {
                                    operations.append(operation(
                                        tag: UInt32(WHISKER_OP_VISIBILITY),
                                        node: fixture.id,
                                        integer: visible ? 1 : 0
                                    ))
                                }
                                if let zOrder = fixture.zOrder {
                                    operations.append(operation(
                                        tag: UInt32(WHISKER_OP_Z_ORDER),
                                        node: fixture.id,
                                        integer: zOrder
                                    ))
                                }
                                let layerRange = layerRanges[index]
                                if !layerRange.isEmpty {
                                    operations.append(operation(
                                        tag: UInt32(WHISKER_OP_BACKGROUND_LAYERS),
                                        node: fixture.id,
                                        payload: UnsafeRawPointer(
                                            backgroundBuffer.baseAddress!.advanced(
                                                by: layerRange.lowerBound
                                            )
                                        ),
                                        count: layerRange.count
                                    ))
                                }
                                let shadowRange = shadowRanges[index]
                                if !shadowRange.isEmpty {
                                    operations.append(operation(
                                        tag: UInt32(WHISKER_OP_BOX_SHADOWS),
                                        node: fixture.id,
                                        payload: UnsafeRawPointer(
                                            shadowBuffer.baseAddress!.advanced(
                                                by: shadowRange.lowerBound
                                            )
                                        ),
                                        count: shadowRange.count
                                    ))
                                }
                                if fixture.clipPath != nil {
                                    operations.append(operation(
                                        tag: UInt32(WHISKER_OP_CLIP_PATH),
                                        node: fixture.id,
                                        payload: UnsafeRawPointer(
                                            clipPaths.advanced(by: index)
                                        ),
                                        count: 1
                                    ))
                                }
                                if let backdropBlur = fixture.backdropBlur {
                                    operations.append(operation(
                                        tag: UInt32(WHISKER_OP_BACKDROP_BLUR),
                                        node: fixture.id,
                                        scalar: backdropBlur
                                    ))
                                }
                                operations.append(operation(
                                    tag: UInt32(WHISKER_OP_IMAGE_RENDERING),
                                    node: fixture.id,
                                    integer: fixture.imageRendering
                                ))
                            }
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
                                frame.operations = UnsafePointer(buffer.baseAddress!)
                                frame.operation_count = buffer.count
                                var response = WhiskerMobileApplyResponse()
                                guard view.applyConformanceFrame(frame, response: &response),
                                      response.status == UInt8(WHISKER_APPLY_ACCEPTED) else {
                                    throw Failure("UIKit Host rejected scene fixture frame")
                                }
                            }
                            }
                            }
                            }
                            }
                        }
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
        setContentScale(surfaceScale, in: view)
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

private func setContentScale(_ scale: CGFloat, in view: UIView) {
    view.contentScaleFactor = scale
    view.setNeedsDisplay()
    view.subviews.forEach { setContentScale(scale, in: $0) }
}

@MainActor
private func awaitResourceState(
    in view: WhiskerView,
    id: UInt64,
    generation: UInt64
) throws -> WhiskerRasterResourceState {
    try awaitResourceState(id: id, generation: generation) {
        view.rasterResourceState(id: id, generation: generation)
    }
}

@MainActor
private func awaitResourceState(
    in service: HostResourceService,
    id: UInt64,
    generation: UInt64
) throws -> WhiskerRasterResourceState {
    try awaitResourceState(id: id, generation: generation) {
        service.state(id: id, generation: generation)
    }
}

@MainActor
private func awaitResourceState(
    id: UInt64,
    generation: UInt64,
    read: () -> WhiskerRasterResourceState?
) throws -> WhiskerRasterResourceState {
    let deadline = Date().addingTimeInterval(5)
    repeat {
        if let state = read(), state != .loading { return state }
        RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(0.01))
    } while Date() < deadline
    throw Failure("timed out waiting for raster resource \(id):\(generation)")
}

private func fixturePNGData() throws -> Data {
    let encoded = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAYAAABytg0kAAAAF0lEQVR4nAXBAQEAAACCIKb33EBkQpUOQdYIeRyCeLsAAAAASUVORK5CYII="
    return try unwrap(Data(base64Encoded: encoded), "fixture PNG")
}

@MainActor
private func dispatchResourceLoad(
    to view: WhiskerView,
    id: UInt64,
    generation: UInt64,
    source: UInt32,
    identifier: String,
    data: Data
) -> Bool {
    let identifierBytes = Array(identifier.utf8CString)
    return identifierBytes.withUnsafeBufferPointer { identifierBuffer in
        data.withUnsafeBytes { dataBuffer in
            var raw = WhiskerMobileResourceCommand()
            raw.command = UInt32(WHISKER_RESOURCE_COMMAND_LOAD)
            raw.kind = UInt32(WHISKER_RESOURCE_RASTER_IMAGE)
            raw.source = source
            raw.resource = id
            raw.generation = generation
            raw.identifier = WhiskerStringRef(
                ptr: identifierBuffer.baseAddress,
                len: identifierBuffer.count - 1
            )
            raw.data = WhiskerBytesRef(
                ptr: dataBuffer.baseAddress?.assumingMemoryBound(to: UInt8.self),
                len: dataBuffer.count
            )
            return withUnsafePointer(to: &raw) { pointer in
                whiskerIOSResourceCommand(Unmanaged.passUnretained(view).toOpaque(), pointer)
            }
        }
    }
}

private struct Pixels {
    let width: Int
    let height: Int
    let bytes: [UInt8]
}

private struct SceneFixtureNode {
    let id: UInt64
    let parent: UInt64?
    let layout: WhiskerMobileLayoutGeometry
    let paint: WhiskerMobileBoxPaint
    let clipFlags: UInt32
    let transform: [Float]?
    let opacity: Float?
    let visible: Bool?
    let zOrder: Int32?
    let text: SceneText?
    let backgroundLayer: SceneBackgroundLayer
    let backgroundLayers: [ScenePaintLayer]
    let boxShadows: [WhiskerMobileBoxShadow]
    let backdropBlur: Float?
    let imageRendering: Int32
    let clipPath: SceneClipPath?
    let linearGradient: SceneLinearGradient?
    let radialGradient: SceneRadialGradient?
    let conicGradient: SceneConicGradient?
}

private struct SceneText {
    let value: String
    let fontSize: Float
    let fontWeight: UInt16
    let color: WhiskerMobileColor
    let alignment: UInt32
    let indentLogicalPixels: Float
    let indentPercentage: Float
    let decorationFlags: UInt32
    let decorationStyle: UInt32
    let decorationColor: WhiskerMobileColor
    let shadowOffset: CGSize?
    let shadowBlurRadius: Float
    let shadowColor: WhiskerMobileColor
}

private struct SceneClipPath {
    let referenceBox: UInt32
    let shapeKind: UInt32
    let inset: WhiskerMobileClipInset
    let circle: WhiskerMobileClipCircle
    let ellipse: WhiskerMobileClipEllipse
    let pathFillRule: UInt32
    let pathCommands: [WhiskerMobilePathCommand]
}

private struct ScenePaintLayer {
    let geometry: SceneBackgroundLayer
    let image: ScenePaintImage
}

private enum ScenePaintImage {
    case resource(UInt64)
    case linear(SceneLinearGradient)
    case radial(SceneRadialGradient)
    case conic(SceneConicGradient)
}

private extension SceneFixtureNode {
    var resolvedBackgroundLayers: [ScenePaintLayer] {
        if !backgroundLayers.isEmpty { return backgroundLayers }
        if let linearGradient {
            return [ScenePaintLayer(geometry: backgroundLayer, image: .linear(linearGradient))]
        }
        if let radialGradient {
            return [ScenePaintLayer(geometry: backgroundLayer, image: .radial(radialGradient))]
        }
        if let conicGradient {
            return [ScenePaintLayer(geometry: backgroundLayer, image: .conic(conicGradient))]
        }
        return []
    }
}

private struct SceneBackgroundLayer {
    let position: [WhiskerMobileLengthPercentage]
    let size: SceneBackgroundSize
    let repeatX: UInt32
    let repeatY: UInt32
    let origin: UInt32
    let clip: UInt32

    static let initial = SceneBackgroundLayer(
        position: [WhiskerMobileLengthPercentage(), WhiskerMobileLengthPercentage()],
        size: .auto,
        repeatX: UInt32(WHISKER_BACKGROUND_REPEAT),
        repeatY: UInt32(WHISKER_BACKGROUND_REPEAT),
        origin: UInt32(WHISKER_BACKGROUND_BOX_PADDING),
        clip: UInt32(WHISKER_BACKGROUND_BOX_BORDER)
    )
}

private enum SceneBackgroundSize {
    case auto
    case explicit(WhiskerMobileLengthPercentage, WhiskerMobileLengthPercentage)
    case cover
    case contain
    case width(WhiskerMobileLengthPercentage)
    case height(WhiskerMobileLengthPercentage)
}

private struct SceneLinearGradient {
    let angleDegrees: Float
    let stops: [WhiskerMobileGradientStop]
}

private struct SceneRadialGradient {
    let center: [Float]
    let radii: [Float]
    let stops: [WhiskerMobileGradientStop]
}

private struct SceneConicGradient {
    let fromDegrees: Float
    let center: [Float]
    let stops: [WhiskerMobileGradientStop]
}

private struct Failure: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}

private func operation(
    tag: UInt32,
    node: UInt64 = 1,
    parent: UInt64 = 0,
    child: UInt64 = 0,
    index: UInt32 = 0,
    member: UInt32 = 0,
    flags: UInt32 = 0,
    integer: Int32 = 0,
    scalar: Float = 0,
    payload: UnsafeRawPointer? = nil,
    count: Int = 0
) -> WhiskerMobileOperation {
    var value = WhiskerMobileOperation()
    value.tag = tag
    value.flags = flags
    value.node = node
    value.parent = parent
    value.child = child
    value.index = index
    value.member = member
    value.integer = integer
    value.scalar = scalar
    value.payload = payload
    value.payload_count = count
    return value
}

private func sceneNode(_ fixture: [String: Any]) throws -> SceneFixtureNode {
    let id = UInt64(try number(fixture, "id"))
    let parent = (fixture["parent"] as? NSNumber).map { UInt64($0.doubleValue) }
    let rect = try numberArray(fixture, "rect")
    guard rect.count == 4 else { throw Failure("scene node rect needs four values") }
    var layout = WhiskerMobileLayoutGeometry()
    layout.border = WhiskerMobileRect(
        x: Float(rect[0]),
        y: Float(rect[1]),
        width: Float(rect[2]),
        height: Float(rect[3])
    )
    let content = try fixture["content_box"].map { _ in
        try numberArray(fixture, "content_box")
    } ?? [0, 0, rect[2], rect[3]]
    guard content.count == 4 else { throw Failure("content box needs four values") }
    layout.content = WhiskerMobileRect(
        x: Float(content[0]),
        y: Float(content[1]),
        width: Float(content[2]),
        height: Float(content[3])
    )
    let clip = fixture["clip"] as? [String: Any]
    let horizontal = try clip.map { try string($0, "horizontal") } ?? "visible"
    let vertical = try clip.map { try string($0, "vertical") } ?? "visible"
    guard horizontal == "visible" || horizontal == "hidden",
          vertical == "visible" || vertical == "hidden" else {
        throw Failure("unknown overflow clip")
    }
    let flags = UInt32(horizontal == "hidden" ? 1 : 0) |
        UInt32(vertical == "hidden" ? 2 : 0)
    let transform: [Float]?
    if let raw = fixture["transform"] as? [NSNumber] {
        guard raw.count == 16 else { throw Failure("transform needs sixteen values") }
        transform = raw.map { $0.floatValue }
    } else {
        transform = nil
    }
    let opacity = (fixture["opacity"] as? NSNumber)?.floatValue
    let visible: Bool?
    if let visibility = fixture["visibility"] as? String {
        guard visibility == "visible" || visibility == "hidden" else {
            throw Failure("unknown visibility")
        }
        visible = visibility == "visible"
    } else {
        visible = nil
    }
    let zOrder: Int32?
    if let number = fixture["z_order"] as? NSNumber {
        let value = number.int64Value
        guard value >= Int64(Int32.min), value <= Int64(Int32.max) else {
            throw Failure("z-order is outside signed 32-bit range")
        }
        zOrder = Int32(value)
    } else {
        zOrder = nil
    }
    let text: SceneText?
    if let raw = fixture["text"] as? [String: Any] {
        let shadow = raw["shadow"] as? [String: Any]
        let decoration = raw["decoration"] as? [String: Any]
        let offset = try shadow.map { try numberArray($0, "offset") }
        let alignment: UInt32
        switch raw["alignment"] as? String ?? "start" {
        case "start": alignment = 0
        case "end": alignment = 1
        case "left": alignment = 2
        case "right": alignment = 3
        case "center": alignment = 4
        default: throw Failure("unknown text alignment")
        }
        let indent = raw["indent"] as? [String: Any]
        text = SceneText(
            value: try string(raw, "value"),
            fontSize: Float(try number(raw, "font_size")),
            fontWeight: UInt16((raw["font_weight"] as? NSNumber)?.intValue ?? 400),
            color: try color(object(raw, "color")),
            alignment: alignment,
            indentLogicalPixels: Float(try indent.map { try number($0, "logical_pixels") } ?? 0),
            indentPercentage: Float(try indent.map { try number($0, "percentage") } ?? 0),
            decorationFlags: try decoration.map {
                switch try string($0, "line") {
                case "underline": 1
                case "line_through": 2
                default: throw Failure("unknown text decoration line")
                }
            } ?? 0,
            decorationStyle: try decoration.map {
                switch try string($0, "style") {
                case "solid": 0
                case "double": 1
                case "dotted": 2
                case "dashed": 3
                case "wavy": 4
                default: throw Failure("unknown text decoration style")
                }
            } ?? 0,
            decorationColor: try decoration.map { try color(object($0, "color")) }
                ?? WhiskerMobileColor(),
            shadowOffset: offset.map { CGSize(width: $0[0], height: $0[1]) },
            shadowBlurRadius: Float(try shadow.map { try number($0, "blur_radius") } ?? 0),
            shadowColor: try shadow.map { try color(object($0, "color")) }
                ?? WhiskerMobileColor()
        )
    } else {
        text = nil
    }
    let backgroundLayer = try fixture["background_layer"]
        .map { try sceneBackgroundLayer($0) } ?? .initial
    let backgroundLayers = try (fixture["background_layers"] as? [[String: Any]] ?? []).map {
        layer -> ScenePaintLayer in
        let geometry = try layer["geometry"].map { try sceneBackgroundLayer($0) } ?? .initial
        let image = try object(layer, "image")
        if let resource = image["resource"] as? NSNumber {
            let id = resource.uint64Value
            guard id != 0 else { throw Failure("resource id must be non-zero") }
            return ScenePaintLayer(geometry: geometry, image: .resource(id))
        }
        if let gradient = image["linear_gradient"] as? [String: Any] {
            return ScenePaintLayer(geometry: geometry, image: .linear(try sceneLinearGradient(gradient)))
        }
        if let gradient = image["radial_gradient"] as? [String: Any] {
            return ScenePaintLayer(geometry: geometry, image: .radial(try sceneRadialGradient(gradient)))
        }
        if let gradient = image["conic_gradient"] as? [String: Any] {
            return ScenePaintLayer(geometry: geometry, image: .conic(try sceneConicGradient(gradient)))
        }
        throw Failure("background layer needs one supported image")
    }
    let boxShadows = try (fixture["box_shadows"] as? [[String: Any]] ?? []).map {
        try sceneBoxShadow($0)
    }
    let backdropBlur = (fixture["backdrop_blur"] as? NSNumber)?.floatValue
    let imageRendering: Int32 = switch fixture["image_rendering"] as? String {
    case "pixelated": Int32(WHISKER_IMAGE_RENDERING_PIXELATED)
    case "crisp_edges": Int32(WHISKER_IMAGE_RENDERING_CRISP_EDGES)
    default: Int32(WHISKER_IMAGE_RENDERING_AUTO)
    }
    let clipPath = try (fixture["clip_path"] as? [String: Any]).map(sceneClipPath)
    let linearGradient: SceneLinearGradient?
    if let gradient = fixture["linear_gradient"] as? [String: Any] {
        linearGradient = try sceneLinearGradient(gradient)
    } else {
        linearGradient = nil
    }
    let radialGradient: SceneRadialGradient?
    if let gradient = fixture["radial_gradient"] as? [String: Any] {
        radialGradient = try sceneRadialGradient(gradient)
    } else {
        radialGradient = nil
    }
    let conicGradient: SceneConicGradient?
    if let gradient = fixture["conic_gradient"] as? [String: Any] {
        conicGradient = try sceneConicGradient(gradient)
    } else {
        conicGradient = nil
    }
    return SceneFixtureNode(
        id: id,
        parent: parent,
        layout: layout,
        paint: try boxPaint(fixture),
        clipFlags: flags,
        transform: transform,
        opacity: opacity,
        visible: visible,
        zOrder: zOrder,
        text: text,
        backgroundLayer: backgroundLayer,
        backgroundLayers: backgroundLayers,
        boxShadows: boxShadows,
        backdropBlur: backdropBlur,
        imageRendering: imageRendering,
        clipPath: clipPath,
        linearGradient: linearGradient,
        radialGradient: radialGradient,
        conicGradient: conicGradient
    )
}

private func sceneClipPath(_ fixture: [String: Any]) throws -> SceneClipPath {
    let shape = try object(fixture, "shape")
    let kind = try string(shape, "kind")
    let referenceBox = try backgroundBox(fixture["reference_box"] as? String ?? "border")
    if kind == "circle" {
        let center = try (shape["center"] as? [Any] ?? []).map(lengthPercentage)
        guard center.count == 2 else { throw Failure("circle needs a center") }
        var circle = WhiskerMobileClipCircle()
        circle.radius = try lengthPercentage(shape["radius"] as Any)
        circle.center_x = center[0]
        circle.center_y = center[1]
        return SceneClipPath(referenceBox: referenceBox, shapeKind: UInt32(WHISKER_CLIP_SHAPE_CIRCLE), inset: WhiskerMobileClipInset(), circle: circle, ellipse: WhiskerMobileClipEllipse(), pathFillRule: 0, pathCommands: [])
    }
    if kind == "ellipse" {
        let radii = try (shape["radii"] as? [Any] ?? []).map(lengthPercentage)
        let center = try (shape["center"] as? [Any] ?? []).map(lengthPercentage)
        guard radii.count == 2, center.count == 2 else { throw Failure("ellipse needs radii and center") }
        var ellipse = WhiskerMobileClipEllipse()
        ellipse.radius_x = radii[0]; ellipse.radius_y = radii[1]
        ellipse.center_x = center[0]; ellipse.center_y = center[1]
        return SceneClipPath(referenceBox: referenceBox, shapeKind: UInt32(WHISKER_CLIP_SHAPE_ELLIPSE), inset: WhiskerMobileClipInset(), circle: WhiskerMobileClipCircle(), ellipse: ellipse, pathFillRule: 0, pathCommands: [])
    }
    if kind == "path" {
        guard let rawCommands = shape["commands"] as? [[String: Any]], !rawCommands.isEmpty else {
            throw Failure("path clip needs commands")
        }
        let commands = try rawCommands.map { raw -> WhiskerMobilePathCommand in
            let name = try string(raw, "command")
            let fields: [String]
            let commandKind: UInt32
            switch name {
            case "move_to": commandKind = UInt32(WHISKER_PATH_MOVE_TO); fields = ["point"]
            case "line_to": commandKind = UInt32(WHISKER_PATH_LINE_TO); fields = ["point"]
            case "quadratic_to": commandKind = UInt32(WHISKER_PATH_QUADRATIC_TO); fields = ["control", "end"]
            case "cubic_to": commandKind = UInt32(WHISKER_PATH_CUBIC_TO); fields = ["control_1", "control_2", "end"]
            case "close": commandKind = UInt32(WHISKER_PATH_CLOSE); fields = []
            default: throw Failure("unsupported path command")
            }
            var points = [WhiskerMobileLengthPercentage](
                repeating: WhiskerMobileLengthPercentage(), count: 6
            )
            var cursor = 0
            for field in fields {
                guard let pair = raw[field] as? [Any], pair.count == 2 else {
                    throw Failure("path point needs two coordinates")
                }
                points[cursor] = try lengthPercentage(pair[0])
                points[cursor + 1] = try lengthPercentage(pair[1])
                cursor += 2
            }
            var command = WhiskerMobilePathCommand()
            command.kind = commandKind
            command.points = (points[0], points[1], points[2], points[3], points[4], points[5])
            return command
        }
        return SceneClipPath(
            referenceBox: referenceBox,
            shapeKind: UInt32(WHISKER_CLIP_SHAPE_PATH),
            inset: WhiskerMobileClipInset(),
            circle: WhiskerMobileClipCircle(),
            ellipse: WhiskerMobileClipEllipse(),
            pathFillRule: (shape["fill_rule"] as? String) == "even_odd"
                ? UInt32(WHISKER_FILL_RULE_EVEN_ODD) : UInt32(WHISKER_FILL_RULE_NON_ZERO),
            pathCommands: commands
        )
    }
    guard kind == "inset" else { throw Failure("unsupported clip-path shape") }
    guard let rawEdges = shape["edges"] as? [Any],
          let rawRadii = shape["radii"] as? [Any] else {
        throw Failure("inset clip-path needs edge and radius arrays")
    }
    let edges = try rawEdges.map(lengthPercentage)
    guard edges.count == 4, rawRadii.count == 4 else {
        throw Failure("inset clip-path needs four edges and radii")
    }
    let radii = try rawRadii.map { value -> (Float, Float) in
        if let number = value as? NSNumber {
            return (number.floatValue, number.floatValue)
        }
        guard let pair = value as? [NSNumber] else {
            throw Failure("clip-path radius must be a number or pair")
        }
        guard pair.count == 2 else { throw Failure("clip-path radius pair needs two values") }
        return (pair[0].floatValue, pair[1].floatValue)
    }
    var inset = WhiskerMobileClipInset()
    inset.edges = (edges[0], edges[1], edges[2], edges[3])
    inset.radii_horizontal = (
        WhiskerMobileLengthPercentage(length: radii[0].0, fraction: 0),
        WhiskerMobileLengthPercentage(length: radii[1].0, fraction: 0),
        WhiskerMobileLengthPercentage(length: radii[2].0, fraction: 0),
        WhiskerMobileLengthPercentage(length: radii[3].0, fraction: 0)
    )
    inset.radii_vertical = (
        WhiskerMobileLengthPercentage(length: radii[0].1, fraction: 0),
        WhiskerMobileLengthPercentage(length: radii[1].1, fraction: 0),
        WhiskerMobileLengthPercentage(length: radii[2].1, fraction: 0),
        WhiskerMobileLengthPercentage(length: radii[3].1, fraction: 0)
    )
    return SceneClipPath(
        referenceBox: referenceBox,
        shapeKind: UInt32(WHISKER_CLIP_SHAPE_INSET),
        inset: inset,
        circle: WhiskerMobileClipCircle(),
        ellipse: WhiskerMobileClipEllipse(),
        pathFillRule: 0,
        pathCommands: []
    )
}

private func sceneBoxShadow(_ fixture: [String: Any]) throws -> WhiskerMobileBoxShadow {
    let offset = try numberArray(fixture, "offset")
    guard offset.count == 2 else { throw Failure("box shadow offset needs two values") }
    var shadow = WhiskerMobileBoxShadow()
    shadow.offset_x = Float(offset[0])
    shadow.offset_y = Float(offset[1])
    shadow.blur_radius = Float(try number(fixture, "blur_radius"))
    shadow.spread_radius = Float(try number(fixture, "spread_radius"))
    shadow.color = try color(try object(fixture, "color"))
    shadow.inset = (fixture["inset"] as? Bool) == true ? 1 : 0
    return shadow
}

private func sceneGradientStops(_ gradient: [String: Any]) throws -> [WhiskerMobileGradientStop] {
    let stops = try objectArray(gradient, "stops").map { stop -> WhiskerMobileGradientStop in
        var raw = WhiskerMobileGradientStop()
        raw.color = try color(try object(stop, "color"))
        raw.position = WhiskerMobileLengthPercentage(
            length: 0,
            fraction: Float(try number(stop, "position"))
        )
        return raw
    }
    guard stops.count >= 2 else { throw Failure("gradient needs at least two stops") }
    return stops
}

private func sceneLinearGradient(_ gradient: [String: Any]) throws -> SceneLinearGradient {
    SceneLinearGradient(
        angleDegrees: Float(try number(gradient, "angle_degrees")),
        stops: try sceneGradientStops(gradient)
    )
}

private func sceneRadialGradient(_ gradient: [String: Any]) throws -> SceneRadialGradient {
    let center = try numberArray(gradient, "center").map(Float.init)
    let radii = try numberArray(gradient, "radii").map(Float.init)
    guard center.count == 2, radii.count == 2 else {
        throw Failure("radial gradient needs a center and two radii")
    }
    return SceneRadialGradient(
        center: center,
        radii: radii,
        stops: try sceneGradientStops(gradient)
    )
}

private func sceneConicGradient(_ gradient: [String: Any]) throws -> SceneConicGradient {
    let center = try numberArray(gradient, "center").map(Float.init)
    guard center.count == 2 else { throw Failure("conic gradient needs a center") }
    return SceneConicGradient(
        fromDegrees: Float(try number(gradient, "from_degrees")),
        center: center,
        stops: try sceneGradientStops(gradient)
    )
}

private func sceneBackgroundLayer(_ value: Any) throws -> SceneBackgroundLayer {
    guard let object = value as? [String: Any] else {
        throw Failure("background_layer must be an object")
    }
    let position = try (object["position"] as? [Any])?
        .map(lengthPercentage) ?? SceneBackgroundLayer.initial.position
    let size = try sceneBackgroundSize(object["size"])
    guard position.count == 2 else { throw Failure("background layer position needs two axes") }
    return SceneBackgroundLayer(
        position: position,
        size: size,
        repeatX: try backgroundRepeat(object["repeat_x"] as? String ?? "repeat"),
        repeatY: try backgroundRepeat(object["repeat_y"] as? String ?? "repeat"),
        origin: try backgroundBox(object["origin"] as? String ?? "padding"),
        clip: try backgroundBox(object["clip"] as? String ?? "border")
    )
}

private func sceneBackgroundSize(_ value: Any?) throws -> SceneBackgroundSize {
    guard let value else { return .auto }
    if let keyword = value as? String {
        return switch keyword {
        case "auto": .auto
        case "cover": .cover
        case "contain": .contain
        default: throw Failure("unsupported background size keyword")
        }
    }
    if let pair = value as? [Any] {
        guard pair.count == 2 else { throw Failure("background size needs two axes") }
        return .explicit(try lengthPercentage(pair[0]), try lengthPercentage(pair[1]))
    }
    if let object = value as? [String: Any] {
        let width = try optionalLengthPercentage(object["width"])
        let height = try optionalLengthPercentage(object["height"])
        switch (width, height) {
        case let (width?, height?): return .explicit(width, height)
        case let (width?, nil): return .width(width)
        case let (nil, height?): return .height(height)
        case (nil, nil): return .auto
        }
    }
    throw Failure("background size must be a keyword, pair, or width/height object")
}

private func optionalLengthPercentage(
    _ value: Any?
) throws -> WhiskerMobileLengthPercentage? {
    guard let value, !(value is NSNull) else { return nil }
    return try lengthPercentage(value)
}

private func lengthPercentage(_ value: Any) throws -> WhiskerMobileLengthPercentage {
    guard let object = value as? [String: Any] else {
        throw Failure("length-percentage must be an object")
    }
    return WhiskerMobileLengthPercentage(
        length: Float(try number(object, "length")),
        fraction: (object["fraction"] as? NSNumber)?.floatValue ?? 0
    )
}

private func backgroundRepeat(_ value: String) throws -> UInt32 {
    switch value {
    case "repeat": UInt32(WHISKER_BACKGROUND_REPEAT)
    case "no_repeat": UInt32(WHISKER_BACKGROUND_NO_REPEAT)
    case "space": UInt32(WHISKER_BACKGROUND_SPACE)
    case "round": UInt32(WHISKER_BACKGROUND_ROUND)
    default: throw Failure("unsupported background repeat")
    }
}

private func backgroundBox(_ value: String) throws -> UInt32 {
    switch value {
    case "border": UInt32(WHISKER_BACKGROUND_BOX_BORDER)
    case "padding": UInt32(WHISKER_BACKGROUND_BOX_PADDING)
    case "content": UInt32(WHISKER_BACKGROUND_BOX_CONTENT)
    case "border_area": UInt32(WHISKER_BACKGROUND_BOX_BORDER_AREA)
    default: throw Failure("unsupported background box")
    }
}

private func boxPaint(_ command: [String: Any]) throws -> WhiskerMobileBoxPaint {
    var value = WhiskerMobileBoxPaint()
    value.background = try color(try object(command, "background"))
    guard let border = command["border"] as? [String: Any] else { return value }
    let widths = try numberArray(border, "widths").map(length)
    let colors = try objectArray(border, "colors").map(color)
    let styles = try stringArray(border, "styles").map(borderStyle)
    let radii = try radiusPairs(border)
    guard widths.count == 4, colors.count == 4, styles.count == 4, radii.count == 4 else {
        throw Failure("border arrays need four values")
    }
    value.widths = (widths[0], widths[1], widths[2], widths[3])
    value.colors = (colors[0], colors[1], colors[2], colors[3])
    value.styles = (styles[0], styles[1], styles[2], styles[3])
    let horizontal = radii.map { length($0.0) }
    let vertical = radii.map { length($0.1) }
    value.radii_horizontal = (horizontal[0], horizontal[1], horizontal[2], horizontal[3])
    value.radii_vertical = (vertical[0], vertical[1], vertical[2], vertical[3])
    return value
}

private func radiusPairs(_ border: [String: Any]) throws -> [(Double, Double)] {
    guard let values = border["radii"] as? [Any] else { throw Failure("missing or invalid radii") }
    return try values.map { value in
        if let number = value as? NSNumber {
            let radius = number.doubleValue
            return (radius, radius)
        }
        if let pair = value as? [NSNumber], pair.count == 2 {
            return (pair[0].doubleValue, pair[1].doubleValue)
        }
        throw Failure("radius must be a number or horizontal/vertical pair")
    }
}

private func color(_ fixture: [String: Any]) throws -> WhiskerMobileColor {
    var value = WhiskerMobileColor()
    let rgba: (UInt8, UInt8, UInt8, Float)
    if try string(fixture, "kind") == "named" {
        switch try string(fixture, "value") {
        case "aqua": rgba = (0, 255, 255, 1)
        case "black": rgba = (0, 0, 0, 1)
        case "blue": rgba = (0, 0, 255, 1)
        case "gray": rgba = (128, 128, 128, 1)
        case "green": rgba = (0, 128, 0, 1)
        case "gold": rgba = (255, 215, 0, 1)
        case "red": rgba = (255, 0, 0, 1)
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
        XCTAssertLessThanOrEqual(
            difference,
            tolerance,
            "\(id) sample (\(x), \(y)): \(actual) != \(expected)"
        )
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

private func containsActiveBackdropBlur(_ view: UIView) -> Bool {
    if let blur = view as? HostBackdropBlurView, !blur.isHidden {
        return true
    }
    return view.subviews.contains(where: containsActiveBackdropBlur)
}

private func findLabel(_ view: UIView) -> UILabel? {
    if let label = view as? UILabel { return label }
    return view.subviews.lazy.compactMap(findLabel).first
}

private func findTextLabels(_ view: UIView) -> [WhiskerTextLabel] {
    var result = view.subviews.flatMap(findTextLabels)
    if let label = view as? WhiskerTextLabel { result.insert(label, at: 0) }
    return result
}

private func containsProjectiveTransform(_ view: UIView) -> Bool {
    let transform = view.layer.transform
    if abs(transform.m14) > 0.000_001 || abs(transform.m24) > 0.000_001 {
        return true
    }
    return view.subviews.contains(where: containsProjectiveTransform)
}

private func luminance(_ color: [UInt8]) -> UInt32 {
    (UInt32(color[0]) * 299 + UInt32(color[1]) * 587 + UInt32(color[2]) * 114) / 1000
}
