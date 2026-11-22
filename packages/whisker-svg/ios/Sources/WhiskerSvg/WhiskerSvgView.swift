// Whisker module view hosting a `WhiskerSvgDrawingView`. Registration is
// driven by `SvgModule`'s `definition()`, not by annotations here.
//
// `WhiskerSvgDrawingView` is a plain view rather than an image view:
// the draw loop is small, and this keeps invalidation under exact
// control instead of behind layer caching.

import Foundation
#if canImport(UIKit)
import UIKit
typealias SvgColor = UIColor
private let defaultTint = UIColor.label
#else
import AppKit
typealias SvgColor = NSColor
private let defaultTint = NSColor.labelColor
#endif
import WhiskerModule

@objc(WhiskerSvgView)
public final class WhiskerSvgView: WhiskerUI<WhiskerSvgDrawingView> {

    @objc public override func createView() -> WhiskerSvgDrawingView {
        let v = WhiskerSvgDrawingView()
        #if canImport(UIKit)
        v.backgroundColor = .clear
        // Repaint on bounds change rather than scaling cached content.
        // The default `.scaleToFill` scales whatever was drawn at the
        // view's FIRST layout, and whisker-router mounts every Switch
        // branch `display:none` before toggling visibility — so that first
        // layout is at zero size, and the empty raster would stay empty
        // forever once the branch is shown (#306).
        v.contentMode = .redraw
        #endif
        return v
    }

    /// Backing of the `_display_list` Prop. The value is the
    /// Rust producer's `whisker_svg::compile()` output, base64
    /// encoded. Empty string → clear the cached bytes (renders
    /// nothing).
    public func setDisplayList(_ base64: String) {
        let v: WhiskerSvgDrawingView = self.view()
        v.displayListBytes = base64.isEmpty ? nil : Data(base64Encoded: base64)
    }

    /// Backing of the `color` Prop. The resolved colour is what the
    /// replayer substitutes wherever the source SVG used
    /// `fill="currentColor"` / `stroke="currentColor"` — the `FILL_TINT` /
    /// `STROKE_TINT` opcodes.
    public func setColor(_ css: String) {
        let v: WhiskerSvgDrawingView = self.view()
        v.tintColorOverride = parseCssColor(css)
    }
}

#if canImport(UIKit)
/// `UIView` that paints the cached display-list bytes inside its own
/// bounds, kept separate from the WhiskerUI bookkeeping because Whisker's UI
/// owner expects a single `view()` accessor.
@objc(WhiskerSvgDrawingView)
public final class WhiskerSvgDrawingView: UIView {

    var displayListBytes: Data? {
        didSet { setNeedsDisplay() }
    }

    /// `.label` puts an unstyled `<Svg>` on the system's primary text colour.
    var tintColorOverride: SvgColor = defaultTint {
        didSet { setNeedsDisplay() }
    }

    public override func layoutSubviews() {
        super.layoutSubviews()
        // A display:none branch is first laid out at zero size, so the raster must redraw at real bounds (#306).
        setNeedsDisplay()
    }

    public override func draw(_ rect: CGRect) {
        guard let ctx = UIGraphicsGetCurrentContext() else { return }
        drawDisplayList(displayListBytes, into: ctx, tint: tintColorOverride, size: bounds.size)
    }
}
#else
@objc(WhiskerSvgDrawingView)
public final class WhiskerSvgDrawingView: NSView {

    var displayListBytes: Data? {
        didSet { needsDisplay = true }
    }

    var tintColorOverride: SvgColor = defaultTint {
        didSet { needsDisplay = true }
    }

    // The display list is authored top-down like UIKit.
    public override var isFlipped: Bool { true }

    public override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        needsDisplay = true
    }

    public override func draw(_ dirtyRect: NSRect) {
        guard let ctx = NSGraphicsContext.current?.cgContext else { return }
        drawDisplayList(displayListBytes, into: ctx, tint: tintColorOverride, size: bounds.size)
    }
}
#endif

private func drawDisplayList(_ bytes: Data?, into ctx: CGContext, tint: SvgColor, size: CGSize) {
    guard let bytes else { return }
    var visitor = CGContextVisitor(context: ctx, tintColor: tint.cgColor, viewSize: size)
    do {
        try dlReplay(bytes, into: &visitor)
    } catch {
        // Draw nothing: the Rust producer guarantees well-formed bytes, so a failure is a Whisker bug, not a reason to crash.
        #if DEBUG
        NSLog("[WhiskerSvg] replay failed: \(error)")
        #endif
    }
}

/// Best-effort CSS colour parser. Supports `#RGB`, `#RRGGBB`,
/// `#RRGGBBAA`, `rgb(…)`, `rgba(…)`, and the small set of named colours
/// the Rust compiler accepts. An unparseable value falls back to `.label`.
private func parseCssColor(_ raw: String) -> SvgColor {
    let s = raw.trimmingCharacters(in: .whitespacesAndNewlines)
    if s.hasPrefix("#") {
        let hex = String(s.dropFirst())
        if hex.count == 3 || hex.count == 6 || hex.count == 8 {
            if let n = UInt32(hex, radix: 16) {
                switch hex.count {
                case 3:
                    let r = (n >> 8) & 0xF
                    let g = (n >> 4) & 0xF
                    let b = n & 0xF
                    return SvgColor(
                        red: CGFloat(r * 16 + r) / 255.0,
                        green: CGFloat(g * 16 + g) / 255.0,
                        blue: CGFloat(b * 16 + b) / 255.0,
                        alpha: 1.0
                    )
                case 6:
                    let r = (n >> 16) & 0xFF
                    let g = (n >> 8) & 0xFF
                    let b = n & 0xFF
                    return SvgColor(
                        red: CGFloat(r) / 255.0,
                        green: CGFloat(g) / 255.0,
                        blue: CGFloat(b) / 255.0,
                        alpha: 1.0
                    )
                case 8:
                    let r = (n >> 24) & 0xFF
                    let g = (n >> 16) & 0xFF
                    let b = (n >> 8) & 0xFF
                    let a = n & 0xFF
                    return SvgColor(
                        red: CGFloat(r) / 255.0,
                        green: CGFloat(g) / 255.0,
                        blue: CGFloat(b) / 255.0,
                        alpha: CGFloat(a) / 255.0
                    )
                default: break
                }
            }
        }
    }
    if let c = parseRgbFunction(s) {
        return c
    }
    switch s.lowercased() {
    case "black": return .black
    case "white": return .white
    case "red": return .red
    case "green": return SvgColor(red: 0, green: 128.0 / 255, blue: 0, alpha: 1)
    case "blue": return .blue
    case "transparent": return .clear
    default: return defaultTint
    }
}

/// Parses `rgb(r, g, b)` / `rgba(r, g, b, a)`, which is what
/// `whisker-css`'s `Color::to_css_string()` emits for every
/// non-hex-literal, non-named colour. Without this branch they fall
/// through to `.label` and the app's colour is silently replaced by the
/// OS semantic one.
private func parseRgbFunction(_ s: String) -> SvgColor? {
    let isRgba = s.hasPrefix("rgba(")
    guard isRgba || s.hasPrefix("rgb(") else { return nil }
    guard s.hasSuffix(")") else { return nil }
    let inner = s.dropFirst(isRgba ? 5 : 4).dropLast()
    let parts = inner.split(separator: ",").map {
        $0.trimmingCharacters(in: .whitespaces)
    }
    guard parts.count == (isRgba ? 4 : 3),
        let r = Double(parts[0]), let g = Double(parts[1]), let b = Double(parts[2])
    else { return nil }
    let a = isRgba ? (Double(parts[3]) ?? 1.0) : 1.0
    return SvgColor(
        red: CGFloat(r) / 255.0,
        green: CGFloat(g) / 255.0,
        blue: CGFloat(b) / 255.0,
        alpha: CGFloat(a)
    )
}
