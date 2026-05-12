import UIKit
// import Lynx — wired up once the pod dependency is resolved.

/// iOS counterpart of LyraView (Android).
///
/// Will inherit from `LynxView` once we wire up the Lynx pod. For the
/// scaffold we leave it as a plain UIView so the Swift package compiles
/// stand-alone.
public final class LyraView: UIView {

    private var nativeHandle: Int = 0

    public override init(frame: CGRect) {
        super.init(frame: frame)
        // Once we have a real LynxView shell pointer we hand it to the Rust
        // runtime here.
        nativeHandle = lyra_runtime_attach_view(/* shellPtr = */ 0)
    }

    public required init?(coder: NSCoder) {
        fatalError("init(coder:) is not supported")
    }

    deinit {
        if nativeHandle != 0 {
            lyra_runtime_destroy(nativeHandle)
        }
    }

    public func onEnterForeground() {
        if nativeHandle != 0 { lyra_runtime_on_enter_foreground(nativeHandle) }
    }

    public func onEnterBackground() {
        if nativeHandle != 0 { lyra_runtime_on_enter_background(nativeHandle) }
    }
}

@_silgen_name("lyra_runtime_attach_view")
func lyra_runtime_attach_view(_ shellPtr: Int) -> Int

@_silgen_name("lyra_runtime_on_enter_foreground")
func lyra_runtime_on_enter_foreground(_ handle: Int)

@_silgen_name("lyra_runtime_on_enter_background")
func lyra_runtime_on_enter_background(_ handle: Int)

@_silgen_name("lyra_runtime_destroy")
func lyra_runtime_destroy(_ handle: Int)
