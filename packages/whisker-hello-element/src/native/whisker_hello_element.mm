// Phase 7-B end-to-end smoke test element.
//
// Registers an `x-hello` tag with Lynx's behaviour registry that
// renders as a plain `UIView` with a system-pink background. Used
// by `examples/hello-world` to validate the tag-by-name dispatch
// path (`whisker_bridge_create_element_by_name` →
// `ElementManager::CreateFiberNode("x-hello")` →
// `LYNX_REGISTER_UI("x-hello")` → `WhiskerHelloElement`).
//
// This is intentionally a single self-contained file in the bridge
// crate, not yet the full `runtime-platforms/ios/` SPM-package
// infrastructure. The latter (with `@WhiskerElement` Swift Macro,
// `WhiskerView<T>` base class, and module-author DX) lands in
// Phase 7-B.6. For the simulator smoke test we use Lynx's raw
// registration macros directly so the element is available
// without further build pipeline changes.

#import <UIKit/UIKit.h>

#import <Lynx/LynxComponentRegistry.h>
#import <Lynx/LynxPropsProcessor.h>
#import <Lynx/LynxUI.h>

@interface WhiskerHelloElement : LynxUI<UIView *>
@end

@implementation WhiskerHelloElement

// Lynx's `+load`-time hook that registers the class against the
// global `LynxComponentRegistry` under the given tag name.
LYNX_REGISTER_UI("x-hello")

- (UIView *)createView {
    UIView *v = [[UIView alloc] init];
    // System pink to make the smoke test visually obvious — if you
    // see a pink rectangle, the tag-by-name dispatch is working
    // end-to-end (render! → Rust → C ABI → Lynx → this class).
    v.backgroundColor = [UIColor systemPinkColor];
    return v;
}

@end
