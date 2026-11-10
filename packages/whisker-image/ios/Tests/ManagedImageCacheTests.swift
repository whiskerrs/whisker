import XCTest
import ImageIO
import CryptoKit
#if canImport(UIKit)
import UIKit
#endif
@testable import WhiskerImage

final class ManagedImageCacheTests: XCTestCase {
    private func png() -> Data {
        let context = CGContext(data: nil, width: 8, height: 6, bitsPerComponent: 8, bytesPerRow: 32,
                                space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        let data = NSMutableData()
        let destination = CGImageDestinationCreateWithData(data, "public.png" as CFString, 1, nil)!
        CGImageDestinationAddImage(destination, context.makeImage()!, nil)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
        return data as Data
    }

    func testSizedDecodeLruAndDiskReuse() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let data = png()
        var requests = 0
        let cache = ManagedImageCache(root: root, fetch: { _, _ in requests += 1; return data })
        let options = #"{"decode_width":4,"decode_height":4}"#
        let a = try cache.load("https://example.com/a", headers: [:], json: options)
        XCTAssertEqual(a.width, 4)
        XCTAssertEqual(a.height, 3)
        let cost = a.bytesPerRow * a.height
        try cache.configure(String(cost * 2))
        let b = try cache.load("https://example.com/b", headers: [:], json: options)
        XCTAssertTrue(a === (try cache.load("https://example.com/a", headers: [:], json: options)))
        _ = try cache.load("https://example.com/c", headers: [:], json: options)
        XCTAssertLessThanOrEqual(cache.retainedBytes, cost * 2)
        XCTAssertTrue(a === (try cache.load("https://example.com/a", headers: [:], json: options)))
        XCTAssertFalse(b === (try cache.load("https://example.com/b", headers: [:], json: options)))
        XCTAssertEqual(requests, 3)
        try cache.configure("0")
        XCTAssertEqual(cache.retainedBytes, 0)
        let reopened = ManagedImageCache(root: root, fetch: { _, _ in XCTFail("disk cache should satisfy load"); return data })
        let original = try reopened.load("https://example.com/a", headers: [:], json: "{}")
        XCTAssertEqual(original.width, 8)
        XCTAssertEqual(original.height, 6)
        try cache.remove("https://example.com/a")
        _ = try cache.load("https://example.com/a", headers: [:], json: options)
        XCTAssertEqual(requests, 4)
        XCTAssertEqual(cache.retainedBytes, 0)
        XCTAssertThrowsError(try cache.load("https://example.com/a", headers: [:], json: #"{"decode_width":0}"#))
    }
    #if canImport(UIKit)
    @MainActor
    func testNativeViewUsesDecodeBoundsAndReportsErrors() throws {
        let url = "https://example.com/" + UUID().uuidString
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("whisker-images")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let digest = SHA256.hash(data: Data(url.utf8)).map { String(format: "%02x", $0) }.joined()
        try png().write(to: root.appendingPathComponent(digest))
        defer { ManagedImageCache.shared.queue.sync { try? ManagedImageCache.shared.remove(url) } }
        let window = UIWindow(frame: UIScreen.main.bounds)
        window.rootViewController = UIViewController()
        let view = WhiskerImageView(frame: CGRect(x: 20, y: 80, width: 200, height: 150))
        window.rootViewController!.view.addSubview(view)
        window.makeKeyAndVisible()
        defer { window.isHidden = true }
        let loaded = expectation(description: "sized image appears")
        view.installWhiskerEventSink { name, _ in
            if name == "load" { loaded.fulfill() }
            if name == "error" { XCTFail("valid file should decode") }
        }
        view.setCache("{\"_src\":\"\(url)\",\"decode_width\":4,\"decode_height\":4}")
        view.setSrc("")
        wait(for: [loaded], timeout: 5)
        XCTAssertEqual(view.view().image?.cgImage?.width, 4)
        XCTAssertEqual(view.view().image?.cgImage?.height, 3)
        let attachment = XCTAttachment(image: UIGraphicsImageRenderer(bounds: window.bounds).image { _ in
            window.drawHierarchy(in: window.bounds, afterScreenUpdates: true)
        })
        attachment.lifetime = .keepAlways
        add(attachment)
        let failed = expectation(description: "invalid decode bound reports an error")
        view.installWhiskerEventSink { name, _ in if name == "error" { failed.fulfill() } }
        view.setCache("{\"_src\":\"\(url)\",\"decode_width\":0}")
        wait(for: [failed], timeout: 5)
        XCTAssertNil(view.view().image)
    }
    #endif

}
