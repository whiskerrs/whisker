import QuartzCore
import UIKit
import WhiskerModule

@_silgen_name("kiba_ios_create")
private func kiba_ios_create(_ layer: UnsafeMutableRawPointer) -> UnsafeMutableRawPointer?

@_silgen_name("kiba_ios_render")
private func kiba_ios_render(_ renderer: UnsafeMutableRawPointer, _ width: UInt32, _ height: UInt32)

@_silgen_name("kiba_ios_destroy")
private func kiba_ios_destroy(_ renderer: UnsafeMutableRawPointer)

public final class KibaSurfaceView: UIView {
    private var renderer: UnsafeMutableRawPointer?
    private var displayLink: CADisplayLink?

    public override class var layerClass: AnyClass { CAMetalLayer.self }

    public override func didMoveToWindow() {
        super.didMoveToWindow()
        displayLink?.invalidate()
        displayLink = nil
        guard let window else { return }
        contentScaleFactor = window.screen.scale
        if renderer == nil {
            renderer = kiba_ios_create(Unmanaged.passUnretained(layer).toOpaque())
        }
        // CADisplayLink retains its target, so it stays invalidated while off-window.
        let link = CADisplayLink(target: self, selector: #selector(renderFrame))
        link.add(to: .main, forMode: .common)
        displayLink = link
    }

    @objc private func renderFrame() {
        guard let renderer else { return }
        let scale = contentScaleFactor
        kiba_ios_render(renderer, UInt32(bounds.width * scale), UInt32(bounds.height * scale))
    }

    deinit {
        if let renderer { kiba_ios_destroy(renderer) }
    }
}

@objc(KibaViewport)
public final class KibaViewport: WhiskerUI<KibaSurfaceView> {
    @objc public override func createView() -> KibaSurfaceView {
        KibaSurfaceView()
    }
}

@WhiskerModule
public final class KibaModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("Kiba")
            View("kiba:Viewport", KibaViewport.self) {}
        }
    }
}
