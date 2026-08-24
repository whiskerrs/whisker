import CoreGraphics

/// Retains decoded Host resources independently from the scene that references them.
final class HostResourceStore {
    private var rasterImages = [UInt64: CGImage]()

    @discardableResult
    func registerRasterImage(_ image: CGImage, id: UInt64) -> Bool {
        guard id != 0, image.width > 0, image.height > 0 else { return false }
        rasterImages[id] = image
        return true
    }

    func rasterImage(id: UInt64) -> CGImage? {
        rasterImages[id]
    }
}
