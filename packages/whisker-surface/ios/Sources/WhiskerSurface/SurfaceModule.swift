import QuartzCore
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif
import WhiskerModule

@_silgen_name("whisker_module_surface_attach")
private func surfaceAttach(
    _ id: UInt64, _ kind: UInt32, _ handle: UnsafeMutableRawPointer,
    _ width: UInt32, _ height: UInt32, _ scale: Float)

@_silgen_name("whisker_module_surface_resize")
private func surfaceResize(_ id: UInt64, _ width: UInt32, _ height: UInt32, _ scale: Float)

@_silgen_name("whisker_module_surface_frame")
private func surfaceFrame(_ id: UInt64, _ timestampSeconds: Double) -> Bool

@_silgen_name("whisker_module_surface_detach")
private func surfaceDetach(_ id: UInt64)

private let coreAnimationLayer: UInt32 = 1

/// Calls Rust only from `WhiskerSurfaceView.onFrame`, never from a prop setter that runs inside Whisker's commit.
public final class WhiskerSurfaceLayerView: WhiskerPlatformView {
    private var surfaceID: UInt64?
    private var attachedID: UInt64?
    private var reportedSize: (UInt32, UInt32, Float)?
    var requestFrame: () -> Void = {}

    private var metalLayer: CAMetalLayer { layer as! CAMetalLayer }

    #if canImport(UIKit)
    public override class var layerClass: AnyClass { CAMetalLayer.self }

    public override init(frame: CGRect) {
        super.init(frame: frame)
        // Presenting inside the Core Animation transaction keeps the drawable in step with Whisker's View updates.
        metalLayer.presentsWithTransaction = true
    }

    public required init?(coder: NSCoder) { nil }

    public override func didMoveToWindow() {
        super.didMoveToWindow()
        windowChanged()
    }

    public override func layoutSubviews() {
        super.layoutSubviews()
        requestFrame()
    }

    private var screenScale: CGFloat { window?.screen.scale ?? contentScaleFactor }
    #else
    public override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
    }

    public required init?(coder: NSCoder) { nil }

    public override func makeBackingLayer() -> CALayer {
        let layer = CAMetalLayer()
        // Presenting inside the Core Animation transaction keeps the drawable in step with Whisker's View updates.
        layer.presentsWithTransaction = true
        return layer
    }

    public override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        windowChanged()
    }

    public override func layout() {
        super.layout()
        requestFrame()
    }

    public override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        requestFrame()
    }

    private var screenScale: CGFloat { window?.backingScaleFactor ?? 1 }
    #endif

    func setSurface(_ value: String) {
        let id = value.split(separator: ":").first.flatMap { UInt64($0) }
        if id != surfaceID {
            detach()
            surfaceID = id
        }
        requestFrame()
    }

    private func windowChanged() {
        guard window != nil else {
            detach()
            return
        }
        requestFrame()
    }

    /// Returns whether the surface wants another frame.
    func frame(_ timestamp: CFTimeInterval) -> Bool {
        guard let id = surfaceID, window != nil else { return false }
        let scale = screenScale
        let width = UInt32(max(0, (bounds.width * scale).rounded()))
        let height = UInt32(max(0, (bounds.height * scale).rounded()))
        guard width > 0, height > 0 else { return false }
        let size = (width, height, Float(scale))
        if attachedID != id {
            metalLayer.contentsScale = scale
            metalLayer.drawableSize = CGSize(width: CGFloat(width), height: CGFloat(height))
            surfaceAttach(id, coreAnimationLayer, Unmanaged.passUnretained(metalLayer).toOpaque(),
                          width, height, Float(scale))
            attachedID = id
            reportedSize = size
        } else if reportedSize.map({ $0 != size }) ?? true {
            metalLayer.contentsScale = scale
            metalLayer.drawableSize = CGSize(width: CGFloat(width), height: CGFloat(height))
            surfaceResize(id, width, height, Float(scale))
            reportedSize = size
        }
        return surfaceFrame(id, timestamp)
    }

    private func detach() {
        if let id = attachedID {
            surfaceDetach(id)
        }
        attachedID = nil
        reportedSize = nil
    }

    deinit {
        if let id = attachedID {
            surfaceDetach(id)
        }
    }
}

@objc(WhiskerSurfaceView)
public final class WhiskerSurfaceView: WhiskerUI<WhiskerSurfaceLayerView> {
    public required init(frame: CGRect) {
        super.init(frame: frame)
        view().requestFrame = { [weak self] in self?.requestFrame() }
    }

    public required init?(coder: NSCoder) { nil }

    @objc public override func createView() -> WhiskerSurfaceLayerView {
        WhiskerSurfaceLayerView(frame: .zero)
    }

    public override func onFrame(timestamp: CFTimeInterval) {
        if view().frame(timestamp) { requestFrame() }
    }
}

@WhiskerModule
public final class SurfaceModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("Surface")
            View("whisker-surface:Surface", WhiskerSurfaceView.self) {
                Prop("surface") { (view: WhiskerSurfaceView, value: WhiskerValue) in
                    view.view().setSurface(value.asString ?? "")
                }
            }
        }
    }
}
