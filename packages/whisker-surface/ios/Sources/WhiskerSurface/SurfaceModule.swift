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

/// Calls Rust only from the display link, never from a prop setter that runs inside Whisker's commit.
public final class WhiskerSurfaceLayerView: WhiskerPlatformView {
    private var surfaceID: UInt64?
    private var attachedID: UInt64?
    private var reportedSize: (UInt32, UInt32, Float)?
    private var link: CADisplayLink?

    private var metalLayer: CAMetalLayer { layer as! CAMetalLayer }

    #if canImport(UIKit)
    public override class var layerClass: AnyClass { CAMetalLayer.self }

    public override func didMoveToWindow() {
        super.didMoveToWindow()
        windowChanged()
    }

    public override func layoutSubviews() {
        super.layoutSubviews()
        schedule()
    }

    private var screenScale: CGFloat { window?.screen.scale ?? contentScaleFactor }
    #else
    public override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
    }

    public required init?(coder: NSCoder) { nil }

    public override func makeBackingLayer() -> CALayer { CAMetalLayer() }

    public override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        windowChanged()
    }

    public override func layout() {
        super.layout()
        schedule()
    }

    public override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        schedule()
    }

    private var screenScale: CGFloat { window?.backingScaleFactor ?? 1 }
    #endif

    func setSurface(_ value: String) {
        let id = value.split(separator: ":").first.flatMap { UInt64($0) }
        if id != surfaceID {
            detach()
            surfaceID = id
        }
        schedule()
    }

    private func windowChanged() {
        link?.invalidate()
        link = nil
        guard window != nil else {
            detach()
            return
        }
        #if canImport(UIKit)
        let link = CADisplayLink(target: WeakTarget(self), selector: #selector(WeakTarget.tick(_:)))
        #else
        let link = displayLink(target: WeakTarget(self), selector: #selector(WeakTarget.tick(_:)))
        #endif
        link.add(to: .main, forMode: .common)
        self.link = link
        schedule()
    }

    private func schedule() {
        link?.isPaused = false
    }

    fileprivate func tick(_ link: CADisplayLink) {
        guard let id = surfaceID else {
            link.isPaused = true
            return
        }
        let scale = screenScale
        let width = UInt32(max(0, (bounds.width * scale).rounded()))
        let height = UInt32(max(0, (bounds.height * scale).rounded()))
        guard width > 0, height > 0 else {
            link.isPaused = true
            return
        }
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
        link.isPaused = !surfaceFrame(id, link.timestamp)
    }

    private func detach() {
        if let id = attachedID {
            surfaceDetach(id)
        }
        attachedID = nil
        reportedSize = nil
    }

    deinit {
        link?.invalidate()
        if let id = attachedID {
            surfaceDetach(id)
        }
    }
}

/// CADisplayLink retains its target, so the view would never deinit without this indirection.
private final class WeakTarget: NSObject {
    weak var view: WhiskerSurfaceLayerView?

    init(_ view: WhiskerSurfaceLayerView) {
        self.view = view
    }

    @objc func tick(_ link: CADisplayLink) {
        guard let view else {
            link.invalidate()
            return
        }
        view.tick(link)
    }
}

@objc(WhiskerSurfaceView)
public final class WhiskerSurfaceView: WhiskerUI<WhiskerSurfaceLayerView> {
    @objc public override func createView() -> WhiskerSurfaceLayerView {
        WhiskerSurfaceLayerView(frame: .zero)
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
