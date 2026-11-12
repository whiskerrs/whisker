import CoreText
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

/** Hand-written native implementations matched to Rust registrations by name. */
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
                label.richOverflow = content.overflow
                label.richParagraph = content.paragraph
                label.richContent = content.paragraph?.attributedString(content.value, attributes: attributes)
                label.attributedText = label.richContent ?? NSAttributedString(
                    string: content.wordBreak == .keepAll
                        ? protectCJKBreaks(content.value) : content.value,
                    attributes: attributes
                )
                label.preparedContent = content.preparedContent
                label.setWhiskerIndent(content.indent)
                label.selectionController?.synchronize()
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
                return scrollView.whiskerContentView
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
) -> (font: WhiskerPlatformFont, family: String) {
    let weight = WhiskerPlatformFont.Weight(rawValue: max(
        -1,
        min(1, CGFloat(fontWeight - 400) / 500)
    ))
    for family in fontFamilies {
        if family == "system" {
            let font = WhiskerPlatformFont.systemFont(ofSize: fontSize, weight: weight)
            return (styledFont(font, style: fontStyle, size: fontSize), "system")
        }
        guard let named = namedFont(family: family, size: fontSize) else { continue }
        let weightedDescriptor = named.fontDescriptor.addingAttributes([
            .traits: [fontWeightTraitKey: weight],
        ])
        let weighted = platformFont(descriptor: weightedDescriptor, size: fontSize) ?? named
        return (styledFont(weighted, style: fontStyle, size: fontSize), family)
    }
    let font = WhiskerPlatformFont.systemFont(ofSize: fontSize, weight: weight)
    return (styledFont(font, style: fontStyle, size: fontSize), "system")
}

#if canImport(UIKit)
private let fontWeightTraitKey = UIFontDescriptor.TraitKey.weight

private func platformFont(descriptor: UIFontDescriptor, size: CGFloat) -> UIFont? {
    UIFont(descriptor: descriptor, size: size)
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
#else
private let fontWeightTraitKey = NSFontDescriptor.TraitKey.weight

private func platformFont(descriptor: NSFontDescriptor, size: CGFloat) -> NSFont? {
    NSFont(descriptor: descriptor, size: size)
}

private func namedFont(family: String, size: CGFloat) -> NSFont? {
    if let exact = NSFont(name: family, size: size) { return exact }
    let members = NSFontManager.shared.availableMembers(ofFontFamily: family) ?? []
    return members.lazy.compactMap { member in
        (member.first as? String).flatMap { NSFont(name: $0, size: size) }
    }.first
}

private func styledFont(
    _ font: NSFont,
    style: WhiskerTextFontStyle,
    size: CGFloat
) -> NSFont {
    guard style != .normal else { return font }
    let descriptor = font.fontDescriptor.withSymbolicTraits(
        font.fontDescriptor.symbolicTraits.union(.italic)
    )
    return NSFont(descriptor: descriptor, size: size) ?? font
}
#endif

private func configuredFont(base: WhiskerPlatformFont, content: WhiskerTextContent) -> WhiskerPlatformFont {
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
    return font as WhiskerPlatformFont
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
            View(WhiskerBuiltInElements.text()) {
                Prop("selectable", clear: { (view: WhiskerTextLabel) in view.setSelectable(false) }) { (view: WhiskerTextLabel, value: WhiskerValue) in
                    view.setSelectable(value.asBool ?? false)
                }
                Command("setSelection") { (view: WhiskerTextLabel, value: WhiskerValue) in view.setTextSelection(value) }
                Command("clearSelection") { (view: WhiskerTextLabel, value: WhiskerValue) in view.clearTextSelection(value) }
                Command("textQuery") { (view: WhiskerTextLabel, value: WhiskerValue) in view.queryText(value) }
                Events("selectionchange", "textqueryresult", "textactivate")
            }
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
                Prop(
                    "show-scroll-indicator",
                    clear: { (view: WhiskerScrollContainerView) in
                        view.setScrollIndicatorVisible(true)
                    }
                ) { (view: WhiskerScrollContainerView, value: WhiskerValue) in
                    view.setScrollIndicatorVisible(value.asBool ?? true)
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
