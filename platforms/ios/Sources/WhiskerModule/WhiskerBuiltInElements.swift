import UIKit

/** Native container whose child geometry is supplied entirely by Rust. */
public class WhiskerContainerView: UIView {
    public override init(frame: CGRect) {
        super.init(frame: frame)
        isOpaque = false
        clipsToBounds = false
    }

    public required init?(coder: NSCoder) { nil }
}

/** Vertical native scroll container with a dedicated multi-child content view. */
public final class WhiskerScrollContainerView: UIScrollView {
    public let contentView = WhiskerContainerView(frame: .zero)

    public override init(frame: CGRect) {
        super.init(frame: frame)
        alwaysBounceVertical = false
        addSubview(contentView)
    }

    public required init?(coder: NSCoder) { nil }

    public override func layoutSubviews() {
        super.layoutSubviews()
        let extent = contentView.subviews.reduce(CGRect.zero) { result, child in
            result.union(child.frame)
        }
        let size = CGSize(
            width: max(bounds.width, extent.maxX),
            height: max(bounds.height, extent.maxY)
        )
        contentSize = size
        contentView.frame = CGRect(origin: .zero, size: size)
    }
}

/** Native text element with the Lynx single-line decoration contract. */
public final class WhiskerTextLabel: UILabel {
    public var whiskerDecoration: WhiskerTextDecoration? {
        didSet { setNeedsDisplay() }
    }

    public override func drawText(in rect: CGRect) {
        super.drawText(in: rect)
        guard let decoration = whiskerDecoration, decoration.style == .wavy else { return }
        let textRect = self.textRect(forBounds: rect, limitedToNumberOfLines: numberOfLines)
        let width = min(textRect.width, sizeThatFits(textRect.size).width)
        guard width > 0, let context = UIGraphicsGetCurrentContext() else { return }
        let stroke = max(1, font.pointSize / 16)
        let baseline = textRect.minY + font.ascender
        let y = decoration.line == .underline
            ? baseline + stroke * 1.5
            : baseline - font.xHeight * 0.45
        context.saveGState()
        context.setStrokeColor(decoration.color.cgColor)
        context.setLineWidth(stroke)
        context.move(to: CGPoint(x: textRect.minX, y: y))
        var x = textRect.minX
        var up = true
        while x < textRect.minX + width {
            x = min(x + stroke * 2, textRect.minX + width)
            context.addLine(to: CGPoint(x: x, y: y + (up ? -stroke : stroke)))
            up.toggle()
        }
        context.strokePath()
        context.restoreGState()
    }
}

/** Hand-written iOS implementations matched to Rust registrations by name. */
public enum WhiskerBuiltInElements {
    public static let viewName = "whisker.ui/View"
    public static let textName = "whisker.ui/Text"
    public static let scrollViewName = "whisker.ui/ScrollView"

    public static func view() -> WhiskerElementFactory {
        WhiskerElementFactory(name: viewName) {
            WhiskerContainerView(frame: .zero)
        }
    }

    public static func text() -> WhiskerElementFactory {
        WhiskerElementFactory(
            name: textName,
            textUpdater: { view, content in
                guard let label = view as? WhiskerTextLabel else {
                    preconditionFailure("\(textName) factory must create WhiskerTextLabel")
                }
                label.font = .systemFont(
                    ofSize: content.fontSize,
                    weight: content.fontWeight >= 600 ? .bold : .regular
                )
                label.textColor = content.color
                label.textAlignment = switch content.alignment {
                case .start: label.effectiveUserInterfaceLayoutDirection == .rightToLeft ? .right : .left
                case .end: label.effectiveUserInterfaceLayoutDirection == .rightToLeft ? .left : .right
                case .left: .left
                case .right: .right
                case .center: .center
                }
                label.whiskerDecoration = content.decoration
                var attributes: [NSAttributedString.Key: Any] = [
                    .font: label.font as Any,
                    .foregroundColor: content.color,
                ]
                if let shadow = content.shadow {
                    let nativeShadow = NSShadow()
                    nativeShadow.shadowOffset = shadow.offset
                    nativeShadow.shadowBlurRadius = shadow.blurRadius
                    nativeShadow.shadowColor = shadow.color
                    attributes[.shadow] = nativeShadow
                }
                if let decoration = content.decoration, decoration.style != .wavy {
                    let style: NSUnderlineStyle = switch decoration.style {
                    case .solid: .single
                    case .double: .double
                    case .dotted: [.single, .patternDot]
                    case .dashed: [.single, .patternDash]
                    case .wavy: []
                    }
                    switch decoration.line {
                    case .underline:
                        attributes[.underlineStyle] = style.rawValue
                        attributes[.underlineColor] = decoration.color
                    case .lineThrough:
                        attributes[.strikethroughStyle] = style.rawValue
                        attributes[.strikethroughColor] = decoration.color
                    }
                }
                label.attributedText = NSAttributedString(
                    string: content.value,
                    attributes: attributes
                )
            }
        ) {
            let label = WhiskerTextLabel(frame: .zero)
            label.numberOfLines = 0
            return label
        }
    }

    public static func scrollView() -> WhiskerElementFactory {
        WhiskerElementFactory(
            name: scrollViewName,
            childrenHost: { view in
                guard let scrollView = view as? WhiskerScrollContainerView else {
                    preconditionFailure(
                        "\(scrollViewName) factory must create WhiskerScrollContainerView"
                    )
                }
                return scrollView.contentView
            }
        ) {
            WhiskerScrollContainerView(frame: .zero)
        }
    }
}

/** Built-ins use exactly the same checked-in ModuleDefinition path as libraries. */
@WhiskerModule
public final class BuiltInElementModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("whisker.ui")
            View(WhiskerBuiltInElements.view())
            View(WhiskerBuiltInElements.text())
            View(WhiskerBuiltInElements.scrollView())
        }
    }
}
