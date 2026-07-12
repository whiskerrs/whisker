// Lynx UI subclass hosting a `WhiskerSvgDrawingView`. A plain
// `WhiskerUI` subclass — no Whisker annotations, registration is
// driven by `SvgModule`'s `definition()`.
//
// The `WhiskerSvgDrawingView` (declared below) is a vanilla UIView
// subclass that overrides `draw(_:)` to decode the cached display
// list bytes into a CGContextVisitor against the active context.
// We don't subclass UIImageView because we don't want any layer
// caching shenanigans — the draw loop is small and we want exact
// control over invalidation.

import Foundation
import UIKit
import WhiskerModule

@objc(WhiskerSvgView)
public final class WhiskerSvgView: WhiskerUI<WhiskerSvgDrawingView> {

    @objc public override func createView() -> WhiskerSvgDrawingView {
        let v = WhiskerSvgDrawingView()
        v.backgroundColor = .clear
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

    /// Backing of the `color` Prop. Parsed as a CSS-style colour
    /// string. The resolved `UIColor` is what the replayer
    /// substitutes wherever the source SVG used
    /// `fill="currentColor"` / `stroke="currentColor"` (= the
    /// `FILL_TINT` / `STROKE_TINT` opcodes).
    public func setColor(_ css: String) {
        let v: WhiskerSvgDrawingView = self.view()
        v.tintColorOverride = parseCssColor(css)
        v.setNeedsDisplay()
    }
}

/// `UIView` that paints the cached display-list bytes inside its
/// own bounds. Lives outside `WhiskerSvgView` because Whisker's UI
/// owner expects a single `view()` accessor — we keep the
/// drawing-specific overrides separate from the LynxUI bookkeeping.
@objc(WhiskerSvgDrawingView)
public final class WhiskerSvgDrawingView: UIView {

    /// Decoded display-list payload. Set by `WhiskerSvgView.setDisplayList(_:)`.
    var displayListBytes: Data? {
        didSet { setNeedsDisplay() }
    }

    /// CSS `color` resolved value used as the tint substitute for
    /// `FILL_TINT` / `STROKE_TINT` opcodes. Default = `.label` so
    /// an unstyled `<Svg>` lands on the system's primary text
    /// colour (matches a typical icon's "I want to be black on
    /// light, white on dark" expectation).
    var tintColorOverride: UIColor = .label {
        didSet { setNeedsDisplay() }
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
            // Malformed stream — fail closed by drawing nothing
            // rather than throwing inside `draw(_:)` (UIKit
            // doesn't propagate). The Rust producer's contract is
            // that bytes are always well-formed; if they're not,
            // that's a Whisker-side bug to surface via diagnostics
            // rather than crash the host.
            #if DEBUG
            NSLog("[WhiskerSvg] replay failed: \(error)")
            #endif
        }
    }
}

/// Best-effort CSS colour parser. Supports `#RGB`, `#RRGGBB`,
/// `#RRGGBBAA`, `rgb(…)`, `rgba(…)`, and the small named colours
/// the Rust compiler accepts. Returns `nil` on parse failure;
/// callers fall back to the previous `tintColor` value.
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

/// Parses `rgb(r, g, b)` / `rgba(r, g, b, a)` — the format
/// `whisker-css`'s `Color::to_css_string()` actually emits for any
/// non-hex-literal, non-named color (e.g. every `Color::hex(...)`
/// constant reactively interpolated into a string, as opposed to a
/// `&'static str` hex literal written by hand). Without this, those
/// colors fell through to the `default: return .label` case above —
/// silently substituting the OS appearance's semantic label color
/// for whatever the app's own color was.
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
