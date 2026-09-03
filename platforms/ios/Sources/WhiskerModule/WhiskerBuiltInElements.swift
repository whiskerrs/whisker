import CoreText
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
public final class WhiskerScrollContainerView: UIScrollView, UIScrollViewDelegate, WhiskerEventSource {
    public let contentView = WhiskerContainerView(frame: .zero)
    private var eventSink: ((String, WhiskerValue) -> Void)?
    private var horizontal = false
    private var chromeVisible = true
    private var snapFactor: CGFloat?
    private var snapOffset: CGFloat = 0
    private var snapStopAlways = false
    private var scrollSequenceStart: CGPoint?

    public override init(frame: CGRect) {
        super.init(frame: frame)
        alwaysBounceVertical = false
        // Whisker lays out against the complete edge-to-edge viewport and
        // exposes safe-area insets separately through the SafeArea module.
        // UIKit's automatic adjustment would add those insets a second time
        // and make the Host disagree with Rust about every child coordinate.
        contentInsetAdjustmentBehavior = .never
        automaticallyAdjustsScrollIndicatorInsets = false
        addSubview(contentView)
        delegate = self
    }

    public required init?(coder: NSCoder) { nil }

    public func installWhiskerEventSink(_ sink: ((String, WhiskerValue) -> Void)?) {
        eventSink = sink
    }

    public func setScrollOrientation(_ value: String) {
        horizontal = value == "horizontal"
        updateIndicatorVisibility()
    }

    public func setWhiskerChromeVisible(_ visible: Bool) {
        chromeVisible = visible
        updateIndicatorVisibility()
    }

    private func updateIndicatorVisibility() {
        showsHorizontalScrollIndicator = chromeVisible && horizontal
        showsVerticalScrollIndicator = chromeVisible && !horizontal
    }

    public func setItemSnap(factor: Double, offset: Double) {
        snapFactor = CGFloat(factor).clamped(to: 0...1)
        snapOffset = CGFloat(offset)
    }

    public func clearItemSnap() {
        snapFactor = nil
        snapOffset = 0
    }

    public func setScrollSnapStop(_ value: String) {
        snapStopAlways = value == "always"
    }

    public func scrollToLogicalOffset(_ offset: Double, smooth: Bool) {
        var target = contentOffset
        if horizontal { target.x = CGFloat(offset) }
        else { target.y = CGFloat(offset) }
        setContentOffset(target, animated: smooth)
    }

    public func scrollByLogicalOffset(_ offset: Double, smooth: Bool) {
        let current = horizontal ? contentOffset.x : contentOffset.y
        scrollToLogicalOffset(Double(current) + offset, smooth: smooth)
    }

    public func scrollViewWillBeginDragging(_ scrollView: UIScrollView) {
        scrollSequenceStart = contentOffset
    }

    public func scrollViewDidScroll(_ scrollView: UIScrollView) {
        eventSink?("scroll", .map([
            "scrollLeft": .float(Double(contentOffset.x)),
            "scrollTop": .float(Double(contentOffset.y)),
            "scrollWidth": .float(Double(contentSize.width)),
            "scrollHeight": .float(Double(contentSize.height)),
            "viewportWidth": .float(Double(bounds.width)),
            "viewportHeight": .float(Double(bounds.height)),
        ]))
    }

    public func scrollViewWillEndDragging(
        _ scrollView: UIScrollView,
        withVelocity velocity: CGPoint,
        targetContentOffset: UnsafeMutablePointer<CGPoint>
    ) {
        guard let factor = snapFactor, !contentView.subviews.isEmpty else { return }
        let viewport = horizontal ? bounds.width : bounds.height
        let contentExtent = horizontal ? contentSize.width : contentSize.height
        let proposed = horizontal ? targetContentOffset.pointee.x : targetContentOffset.pointee.y
        let maximum = max(0, contentExtent - viewport)
        let targets = contentView.subviews
            .map { child -> CGFloat in
                let frame = child.frame
                let start = horizontal ? frame.minX : frame.minY
                let size = horizontal ? frame.width : frame.height
                return (start + size * factor - viewport * factor + snapOffset)
                    .clamped(to: 0...maximum)
            }
            .sorted()
        let startPoint = scrollSequenceStart ?? contentOffset
        let start = horizontal ? startPoint.x : startPoint.y
        let target: CGFloat
        if snapStopAlways, proposed > start + .ulpOfOne {
            target = targets.first(where: { $0 > start + .ulpOfOne }) ?? targets.last ?? proposed
        } else if snapStopAlways, proposed < start - .ulpOfOne {
            target = targets.last(where: { $0 < start - .ulpOfOne }) ?? targets.first ?? proposed
        } else {
            target = targets.min(by: { abs($0 - proposed) < abs($1 - proposed) }) ?? proposed
        }
        scrollSequenceStart = nil
        if horizontal { targetContentOffset.pointee.x = target }
        else { targetContentOffset.pointee.y = target }
    }

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

/** Native text element implementing Whisker's single-line decoration contract. */
public final class WhiskerTextLabel: UILabel {
    private var whiskerIndent = WhiskerTextIndent()
    private var appliedIndent: CGFloat?
    public internal(set) var whiskerFontFeatures: [WhiskerFontFeature] = []
    public internal(set) var whiskerFontVariations: [WhiskerFontVariation] = []
    public internal(set) var whiskerFontOpticalSizing: WhiskerFontOpticalSizing = .none
    public internal(set) var whiskerFontFamilies: [String] = ["system"]
    public internal(set) var whiskerResolvedFontFamily = ""
    public internal(set) var whiskerFontWeight = 400
    public internal(set) var whiskerFontStyle: WhiskerTextFontStyle = .normal
    public internal(set) var whiskerLineHeight: CGFloat?
    public internal(set) var whiskerLetterSpacing: CGFloat = 0
    public internal(set) var whiskerDirection: WhiskerTextDirection = .auto

    public func setWhiskerIndent(_ indent: WhiskerTextIndent) {
        whiskerIndent = indent
        appliedIndent = nil
        applyWhiskerIndent()
    }

    public override func layoutSubviews() {
        super.layoutSubviews()
        applyWhiskerIndent()
    }

    private func applyWhiskerIndent() {
        let resolved = whiskerIndent.resolve(width: bounds.width)
        guard appliedIndent != resolved, let attributedText else { return }
        appliedIndent = resolved
        let mutable = NSMutableAttributedString(attributedString: attributedText)
        let existingParagraph = attributedText.length > 0
            ? attributedText.attribute(.paragraphStyle, at: 0, effectiveRange: nil)
                as? NSParagraphStyle
            : nil
        let paragraph = existingParagraph?.mutableCopy() as? NSMutableParagraphStyle
            ?? NSMutableParagraphStyle()
        paragraph.alignment = textAlignment
        paragraph.lineBreakMode = lineBreakMode
        paragraph.firstLineHeadIndent = resolved
        mutable.addAttribute(.paragraphStyle, value: paragraph, range: NSRange(location: 0, length: mutable.length))
        self.attributedText = mutable
    }

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
                let base = resolveWhiskerBaseFont(
                    fontFamilies: content.fontFamilies,
                    fontSize: content.fontSize,
                    fontWeight: content.fontWeight,
                    fontStyle: content.fontStyle
                )
                label.font = configuredFont(base: base.font, content: content)
                label.whiskerFontFamilies = content.fontFamilies
                label.whiskerResolvedFontFamily = base.family
                label.whiskerFontWeight = content.fontWeight
                label.whiskerFontStyle = content.fontStyle
                label.whiskerLineHeight = content.lineHeight
                label.whiskerLetterSpacing = content.letterSpacing
                label.whiskerFontFeatures = content.fontFeatures
                label.whiskerFontVariations = content.fontVariations
                label.whiskerFontOpticalSizing = content.fontOpticalSizing
                label.textColor = content.color
                label.whiskerDirection = content.direction
                label.semanticContentAttribute = switch content.direction {
                case .auto: .unspecified
                case .leftToRight: .forceLeftToRight
                case .rightToLeft: .forceRightToLeft
                }
                label.textAlignment = switch content.alignment {
                case .start:
                    switch content.direction {
                    case .auto:
                        label.effectiveUserInterfaceLayoutDirection == .rightToLeft
                            ? .right : .left
                    case .leftToRight: .left
                    case .rightToLeft: .right
                    }
                case .end:
                    switch content.direction {
                    case .auto:
                        label.effectiveUserInterfaceLayoutDirection == .rightToLeft
                            ? .left : .right
                    case .leftToRight: .right
                    case .rightToLeft: .left
                    }
                case .left: .left
                case .right: .right
                case .center: .center
                }
                label.whiskerDecoration = content.decoration
                label.numberOfLines = content.wrap
                    ? (content.maxLines == 0 ? 0 : content.maxLines)
                    : 1
                label.lineBreakMode = switch content.overflow {
                case .ellipsis: .byTruncatingTail
                case .clip: content.wordBreak == .breakAll ? .byCharWrapping : .byWordWrapping
                }
                let paragraph = NSMutableParagraphStyle()
                paragraph.alignment = label.textAlignment
                paragraph.baseWritingDirection = switch content.direction {
                case .auto: .natural
                case .leftToRight: .leftToRight
                case .rightToLeft: .rightToLeft
                }
                paragraph.lineBreakMode = label.lineBreakMode
                if let lineHeight = content.lineHeight {
                    paragraph.minimumLineHeight = lineHeight
                    paragraph.maximumLineHeight = lineHeight
                }
                var attributes: [NSAttributedString.Key: Any] = [
                    .font: label.font as Any,
                    .foregroundColor: content.color,
                    .kern: content.letterSpacing,
                    .paragraphStyle: paragraph,
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
                    string: content.wordBreak == .keepAll
                        ? protectCJKBreaks(content.value) : content.value,
                    attributes: attributes
                )
                label.setWhiskerIndent(content.indent)
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

public func resolveWhiskerBaseFont(
    fontFamilies: [String],
    fontSize: CGFloat,
    fontWeight: Int,
    fontStyle: WhiskerTextFontStyle
) -> (font: UIFont, family: String) {
    let weight = UIFont.Weight(rawValue: max(
        -1,
        min(1, CGFloat(fontWeight - 400) / 500)
    ))
    for family in fontFamilies {
        if family == "system" {
            let font = UIFont.systemFont(ofSize: fontSize, weight: weight)
            return (styledFont(font, style: fontStyle, size: fontSize), "system")
        }
        guard let named = namedFont(family: family, size: fontSize) else { continue }
        let weightedDescriptor = named.fontDescriptor.addingAttributes([
            .traits: [UIFontDescriptor.TraitKey.weight: weight],
        ])
        let weighted = UIFont(descriptor: weightedDescriptor, size: fontSize)
        return (styledFont(weighted, style: fontStyle, size: fontSize), family)
    }
    let font = UIFont.systemFont(ofSize: fontSize, weight: weight)
    return (styledFont(font, style: fontStyle, size: fontSize), "system")
}

private func namedFont(family: String, size: CGFloat) -> UIFont? {
    if let exact = UIFont(name: family, size: size) { return exact }
    return UIFont.fontNames(forFamilyName: family).lazy.compactMap {
        UIFont(name: $0, size: size)
    }.first
}

private func styledFont(
    _ font: UIFont,
    style: WhiskerTextFontStyle,
    size: CGFloat
) -> UIFont {
    guard style != .normal,
          let descriptor = font.fontDescriptor.withSymbolicTraits(
              font.fontDescriptor.symbolicTraits.union(.traitItalic)
          ) else { return font }
    return UIFont(descriptor: descriptor, size: size)
}

private func configuredFont(base: UIFont, content: WhiskerTextContent) -> UIFont {
    var font = base as CTFont
    var attributes: [CFString: Any] = [:]
    if !content.fontFeatures.isEmpty {
        let settings: [[CFString: Any]] = content.fontFeatures.map { feature in
            [
                kCTFontOpenTypeFeatureTag: openTypeCode(feature.tag),
                kCTFontOpenTypeFeatureValue: feature.value,
            ]
        }
        attributes[kCTFontFeatureSettingsAttribute] = settings
    }
    var variations = Dictionary(uniqueKeysWithValues: content.fontVariations.map {
        (NSNumber(value: openTypeCode($0.tag)), NSNumber(value: Double($0.value)))
    })
    if content.fontOpticalSizing == .auto,
       !content.fontVariations.contains(where: { $0.tag == "opsz" }) {
        variations[NSNumber(value: openTypeCode("opsz"))] = NSNumber(value: Double(content.fontSize))
    }
    if !variations.isEmpty {
        attributes[kCTFontVariationAttribute] = variations
    }
    if !attributes.isEmpty {
        let descriptor = CTFontDescriptorCreateWithAttributes(attributes as CFDictionary)
        font = CTFontCreateCopyWithAttributes(font, content.fontSize, nil, descriptor)
    }
    return font as UIFont
}

private func openTypeCode(_ tag: String) -> UInt32 {
    tag.utf8.reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
}

private func protectCJKBreaks(_ value: String) -> String {
    var result = ""
    var previousWasCJK = false
    for character in value {
        let currentIsCJK = character.unicodeScalars.contains(where: isCJK)
        if previousWasCJK && currentIsCJK { result.append("\u{2060}") }
        result.append(character)
        previousWasCJK = currentIsCJK
    }
    return result
}

private func isCJK(_ scalar: UnicodeScalar) -> Bool {
    (0x2E80...0x9FFF).contains(scalar.value)
        || (0xF900...0xFAFF).contains(scalar.value)
        || (0xAC00...0xD7AF).contains(scalar.value)
}

/** Built-ins use exactly the same checked-in ModuleDefinition path as libraries. */
@WhiskerModule
public final class BuiltInElementModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("whisker.ui")
            View(WhiskerBuiltInElements.view())
            View(WhiskerBuiltInElements.text())
            View(WhiskerBuiltInElements.scrollView()) {
                Prop(
                    "scroll-orientation",
                    clear: { (view: WhiskerScrollContainerView) in
                        view.setScrollOrientation("vertical")
                    }
                ) { (view: WhiskerScrollContainerView, value: WhiskerValue) in
                    view.setScrollOrientation(value.asString ?? "vertical")
                }
                Prop(
                    "item-snap",
                    clear: { (view: WhiskerScrollContainerView) in view.clearItemSnap() }
                ) { (view: WhiskerScrollContainerView, value: WhiskerValue) in
                    guard case .map(let snap) = value else {
                        view.clearItemSnap()
                        return
                    }
                    view.setItemSnap(
                        factor: snap["factor"]?.asDouble ?? 0,
                        offset: snap["offset"]?.asDouble ?? 0
                    )
                }
                Prop(
                    "scroll-snap-stop",
                    clear: { (view: WhiskerScrollContainerView) in
                        view.setScrollSnapStop("normal")
                    }
                ) { (view: WhiskerScrollContainerView, value: WhiskerValue) in
                    view.setScrollSnapStop(value.asString ?? "normal")
                }
                Prop(
                    "enable-scroll",
                    clear: { (view: WhiskerScrollContainerView) in
                        view.isScrollEnabled = true
                    }
                ) { (view: WhiskerScrollContainerView, value: WhiskerValue) in
                    view.isScrollEnabled = value.asBool ?? true
                }
                Command("scrollTo") { (view: WhiskerScrollContainerView, value: WhiskerValue) in
                    guard case .map(let arguments) = value else { return }
                    view.scrollToLogicalOffset(
                        arguments["offset"]?.asDouble ?? 0,
                        smooth: arguments["smooth"]?.asBool ?? false
                    )
                }
                Command("scrollBy") { (view: WhiskerScrollContainerView, value: WhiskerValue) in
                    guard case .map(let arguments) = value else { return }
                    view.scrollByLogicalOffset(
                        arguments["offset"]?.asDouble ?? 0,
                        smooth: arguments["smooth"]?.asBool ?? false
                    )
                }
                Events("scroll")
            }
        }
    }
}

private extension Comparable {
    func clamped(to range: ClosedRange<Self>) -> Self {
        min(max(self, range.lowerBound), range.upperBound)
    }
}
