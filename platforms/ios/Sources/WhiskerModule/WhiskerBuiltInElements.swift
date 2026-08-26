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
                guard let label = view as? UILabel else {
                    preconditionFailure("\(textName) factory must create UILabel")
                }
                label.font = .systemFont(
                    ofSize: content.fontSize,
                    weight: content.fontWeight >= 600 ? .bold : .regular
                )
                label.textColor = content.color
                if let shadow = content.shadow {
                    let nativeShadow = NSShadow()
                    nativeShadow.shadowOffset = shadow.offset
                    nativeShadow.shadowBlurRadius = shadow.blurRadius
                    nativeShadow.shadowColor = shadow.color
                    label.attributedText = NSAttributedString(
                        string: content.value,
                        attributes: [
                            .font: label.font as Any,
                            .foregroundColor: content.color,
                            .shadow: nativeShadow,
                        ]
                    )
                } else {
                    label.attributedText = nil
                    label.text = content.value
                }
            }
        ) {
            let label = UILabel(frame: .zero)
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
