// Whisker module view hosting a `WhiskerSvgDrawingView`. Registration is
// driven by `SvgModule`'s `definition()`, not by annotations here.
//
// `WhiskerSvgDrawingView` is a plain UIView rather than a UIImageView:
// the draw loop is small, and this keeps invalidation under exact
// control instead of behind layer caching.

import Foundation
import UIKit
import WhiskerModule

@objc(WhiskerSvgView)
public final class WhiskerSvgView: WhiskerUI<WhiskerSvgDrawingView> {

    @objc public override func createView() -> WhiskerSvgDrawingView {
        let v = WhiskerSvgDrawingView()
        v.backgroundColor = .clear
        // Repaint on bounds change rather than scaling cached content.
        // The default `.scaleToFill` scales whatever was drawn at the
        // view's FIRST layout, and whisker-router mounts every Switch
        // branch `display:none` before toggling visibility — so that first
        // layout is at zero size, and the empty raster would stay empty
        // forever once the branch is shown (#306).
        v.contentMode = .redraw
        return v
    }

    /// Backing of the `_display_list` Prop. The value is the
    /// Rust producer's `whisker_svg::compile()` output, base64
    /// encoded. Empty string → clear the cached bytes (renders
    /// nothing).
    public func setDisplayList(_ base64: String) {
        let v: WhiskerSvgDrawingView = self.view()
        if base64.isEmpty {
            v.displayListBytes = nil
            v.setNeedsDisplay()
            return
        }
        guard let data = Data(base64Encoded: base64) else {
            v.displayListBytes = nil
            v.setNeedsDisplay()
            return
        }
        v.displayListBytes = data
        v.setNeedsDisplay()
    }

    /// Backing of the `color` Prop. The resolved `UIColor` is what the
    /// replayer substitutes wherever the source SVG used
    /// `fill="currentColor"` / `stroke="currentColor"` — the `FILL_TINT` /
    /// `STROKE_TINT` opcodes.
    public func setColor(_ css: String) {
        let v: WhiskerSvgDrawingView = self.view()
        v.tintColorOverride = parseCssColor(css)
        v.setNeedsDisplay()
    }
}

/// `UIView` that paints the cached display-list bytes inside its own
/// bounds, kept separate from the WhiskerUI bookkeeping because Whisker's UI
/// owner expects a single `view()` accessor.
@objc(WhiskerSvgDrawingView)
public final class WhiskerSvgDrawingView: UIView {

    /// Decoded display-list payload. Set by `WhiskerSvgView.setDisplayList(_:)`.
    var displayListBytes: Data? {
        didSet { setNeedsDisplay() }
    }

    /// Tint substitute for the `FILL_TINT` / `STROKE_TINT` opcodes.
    /// Defaulting to `.label` puts an unstyled `<Svg>` on the system's
    /// primary text colour — black on light, white on dark, which is what
    /// an icon usually wants.
    var tintColorOverride: UIColor = .label {
        didSet { setNeedsDisplay() }
    }

    public override func layoutSubviews() {
        super.layoutSubviews()
        // Required for the display:none-branch case (#306): the view is
        // first laid out at zero size and gets real bounds only when the
        // branch becomes visible, so `draw(_:)` has to re-run at the new
        // size rather than reuse the empty raster.
        setNeedsDisplay()
    }

    public override func draw(_ rect: CGRect) {
        guard let bytes = displayListBytes,
              let ctx = UIGraphicsGetCurrentContext()
        else { return }
        var visitor = CGContextVisitor(
            context: ctx,
            tintColor: tintColorOverride,
            viewSize: bounds.size
        )
        do {
            try dlReplay(bytes, into: &visitor)
        } catch {
            // Fail closed by drawing nothing: UIKit doesn't propagate a
            // throw out of `draw(_:)`, and the Rust producer's contract is
            // that the bytes are well-formed — a violation is a
            // Whisker-side bug to diagnose, not a reason to kill the host.
            #if DEBUG
            NSLog("[WhiskerSvg] replay failed: \(error)")
            #endif
        }
    }
}

/// Best-effort CSS colour parser. Supports `#RGB`, `#RRGGBB`,
/// `#RRGGBBAA`, `rgb(…)`, `rgba(…)`, and the small set of named colours
/// the Rust compiler accepts. An unparseable value falls back to `.label`.
private func parseCssColor(_ raw: String) -> UIColor {
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
                    return UIColor(
                        red: CGFloat(r * 16 + r) / 255.0,
                        green: CGFloat(g * 16 + g) / 255.0,
                        blue: CGFloat(b * 16 + b) / 255.0,
                        alpha: 1.0
                    )
                case 6:
                    let r = (n >> 16) & 0xFF
                    let g = (n >> 8) & 0xFF
                    let b = n & 0xFF
                    return UIColor(
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
                    return UIColor(
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
    case "green": return UIColor(red: 0, green: 128.0 / 255, blue: 0, alpha: 1)
    case "blue": return .blue
    case "transparent": return .clear
    default: return .label
    }
}

/// Parses `rgb(r, g, b)` / `rgba(r, g, b, a)`, which is what
/// `whisker-css`'s `Color::to_css_string()` emits for every
/// non-hex-literal, non-named colour. Without this branch they fall
/// through to `.label` and the app's colour is silently replaced by the
/// OS semantic one.
private func parseRgbFunction(_ s: String) -> UIColor? {
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
    return UIColor(
        red: CGFloat(r) / 255.0,
        green: CGFloat(g) / 255.0,
        blue: CGFloat(b) / 255.0,
        alpha: CGFloat(a)
    )
}
