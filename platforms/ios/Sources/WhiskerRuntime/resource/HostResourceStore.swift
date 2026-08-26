import CoreGraphics

struct HostRasterResource {
    let image: CGImage
    let intrinsicSize: CGSize
}

/// Retains decoded Host resources independently from the scene that references them.
final class HostResourceStore {
    private var rasterImages = [UInt64: HostRasterResource]()

    @discardableResult
    func registerRasterImage(_ image: CGImage, id: UInt64) -> Bool {
        guard id != 0, image.width > 0, image.height > 0 else { return false }
        rasterImages[id] = HostRasterResource(
            image: image,
            intrinsicSize: CGSize(width: image.width, height: image.height)
        )
        return true
    }

    func rasterImage(id: UInt64) -> CGImage? {
        rasterImages[id]?.image
    }

    func rasterResource(id: UInt64) -> HostRasterResource? {
        rasterImages[id]
    }

    func removeRasterImage(id: UInt64) {
        rasterImages.removeValue(forKey: id)
    }
}
