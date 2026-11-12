// `CGContextVisitor` — paints the display-list stream into a
// `CGContext`. Implements the same `DLVisitor` protocol as the
// trace recorder, so the byte decoder code path is identical
// between unit tests and production drawing.
//
// Paint state model — see SPEC.md §"Tint propagation":
//
// * `FILL_COLOR` / `STROKE_COLOR` set a literal colour AND clear
//   the corresponding tint flag.
// * `FILL_TINT` / `STROKE_TINT` set the tint flag — the actual
//   paint substituted at fill/stroke time is the host's
//   `tintColor`.
// * `SAVE` / `RESTORE` push and pop both our tint flags AND
//   delegate to `CGContextSaveGState` so CGContext-managed state
//   (alpha, line width, transform, current path) round-trips too.

import CoreGraphics

struct CGContextVisitor: DLVisitor {
    private let cg: CGContext
    private let tintColor: CGColor
    private let viewSize: CGSize

    // Our own paint state — CGContext doesn't model "use tint
    // at fill time" semantics.
    private var currentFill: CGColor? = CGColor(red: 0, green: 0, blue: 0, alpha: 1)
    private var currentStroke: CGColor? = nil
    private var fillIsTint = false
    private var strokeIsTint = false
    /// Stack pushed on `SAVE`, popped on `RESTORE` so paint state
    /// round-trips through nested groups just like CGContext's
    /// own saveGState handles CTM / alpha / line width.
    private var savedPaintStates: [PaintSnapshot] = []
    private var currentPath: CGMutablePath?

    private struct PaintSnapshot {
        let fill: CGColor?
        let stroke: CGColor?
        let fillIsTint: Bool
        let strokeIsTint: Bool
    }

    init(context: CGContext, tintColor: CGColor, viewSize: CGSize) {
        self.cg = context
        self.tintColor = tintColor
        self.viewSize = viewSize
    }

    // ---- container / state ------------------------------------------------

    mutating func save() {
        cg.saveGState()
        savedPaintStates.append(PaintSnapshot(
            fill: currentFill,
            stroke: currentStroke,
            fillIsTint: fillIsTint,
            strokeIsTint: strokeIsTint
        ))
    }

    mutating func restore() {
        cg.restoreGState()
        if let s = savedPaintStates.popLast() {
            currentFill = s.fill
            currentStroke = s.stroke
            fillIsTint = s.fillIsTint
            strokeIsTint = s.strokeIsTint
        }
    }

    mutating func concat(_ t: DLTransform) {
        cg.concatenate(CGAffineTransform(
            a: CGFloat(t.a), b: CGFloat(t.b),
            c: CGFloat(t.c), d: CGFloat(t.d),
            tx: CGFloat(t.tx), ty: CGFloat(t.ty)
        ))
    }

    mutating func viewport(minX: Float, minY: Float, width: Float, height: Float) {
        // preserveAspectRatio="xMidYMid meet" — see SPEC.md
        // §"Viewport mapping". Note we don't push this via SAVE /
        // RESTORE because the SPEC mandates one VIEWPORT op at
        // body start and forbids further appearance.
        let vw = CGFloat(width)
        let vh = CGFloat(height)
        if vw <= 0 || vh <= 0 { return }
        let sx = viewSize.width / vw
        let sy = viewSize.height / vh
        let scale = min(sx, sy)
        let tx = (viewSize.width - vw * scale) / 2 - CGFloat(minX) * scale
        let ty = (viewSize.height - vh * scale) / 2 - CGFloat(minY) * scale
        cg.concatenate(CGAffineTransform(a: scale, b: 0, c: 0, d: scale, tx: tx, ty: ty))
    }

    // ---- paint state ------------------------------------------------------

    mutating func fillColor(_ c: DLColor) {
        currentFill = cgColor(c)
        fillIsTint = false
    }

    mutating func strokeColor(_ c: DLColor) {
        currentStroke = cgColor(c)
        strokeIsTint = false
    }

    mutating func strokeWidth(_ w: Float) {
        cg.setLineWidth(CGFloat(w))
    }

    mutating func opacity(_ a: Float) {
        cg.setAlpha(CGFloat(a))
    }

    mutating func fillTint() {
        fillIsTint = true
    }

    mutating func strokeTint() {
        strokeIsTint = true
    }

    // ---- path -------------------------------------------------------------

    mutating func pathBegin() {
        currentPath = CGMutablePath()
    }

    mutating func moveTo(x: Float, y: Float) {
        ensurePath().move(to: CGPoint(x: CGFloat(x), y: CGFloat(y)))
    }

    mutating func lineTo(x: Float, y: Float) {
        ensurePath().addLine(to: CGPoint(x: CGFloat(x), y: CGFloat(y)))
    }

    mutating func quadTo(cx: Float, cy: Float, x: Float, y: Float) {
        ensurePath().addQuadCurve(
            to: CGPoint(x: CGFloat(x), y: CGFloat(y)),
            control: CGPoint(x: CGFloat(cx), y: CGFloat(cy))
        )
    }

    mutating func cubicTo(c1x: Float, c1y: Float, c2x: Float, c2y: Float, x: Float, y: Float) {
        ensurePath().addCurve(
            to: CGPoint(x: CGFloat(x), y: CGFloat(y)),
            control1: CGPoint(x: CGFloat(c1x), y: CGFloat(c1y)),
            control2: CGPoint(x: CGFloat(c2x), y: CGFloat(c2y))
        )
    }

    mutating func close() {
        ensurePath().closeSubpath()
    }

    mutating func fill() {
        guard let path = currentPath else { return }
        let resolved = fillIsTint ? tintColor : currentFill
        guard let color = resolved else { return }
        cg.addPath(path)
        cg.setFillColor(color)
        cg.fillPath()
    }

    mutating func stroke() {
        guard let path = currentPath else { return }
        let resolved = strokeIsTint ? tintColor : currentStroke
        guard let color = resolved else { return }
        cg.addPath(path)
        cg.setStrokeColor(color)
        cg.strokePath()
    }

    mutating func fillAndStroke() {
        guard let path = currentPath else { return }
        let fillResolved = fillIsTint ? tintColor : currentFill
        let strokeResolved = strokeIsTint ? tintColor : currentStroke
        // Combined drawPath is the cheapest CoreGraphics path; fall
        // back to two separate draws when only one paint side is
        // present.
        if let fc = fillResolved, let sc = strokeResolved {
            cg.addPath(path)
            cg.setFillColor(fc)
            cg.setStrokeColor(sc)
            cg.drawPath(using: .fillStroke)
        } else if let fc = fillResolved {
            cg.addPath(path)
            cg.setFillColor(fc)
            cg.fillPath()
        } else if let sc = strokeResolved {
            cg.addPath(path)
            cg.setStrokeColor(sc)
            cg.strokePath()
        }
    }

    // ---- helpers ----------------------------------------------------------

    private mutating func ensurePath() -> CGMutablePath {
        if let p = currentPath { return p }
        let p = CGMutablePath()
        currentPath = p
        return p
    }

    private func cgColor(_ c: DLColor) -> CGColor {
        CGColor(
            red: CGFloat(c.r) / 255.0,
            green: CGFloat(c.g) / 255.0,
            blue: CGFloat(c.b) / 255.0,
            alpha: CGFloat(c.a) / 255.0
        )
    }
}
