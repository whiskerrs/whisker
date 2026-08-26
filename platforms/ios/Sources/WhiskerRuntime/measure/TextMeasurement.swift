import CoreText
import UIKit
import WhiskerModule

let whiskerIOSMeasure: WhiskerMeasureHost = { data, requests, count, responses in
    guard data != nil, let requests, let responses else { return false }
    var textFontFamilies = [[String]?](repeating: nil, count: count)
    for index in 0..<count {
        let request = requests.advanced(by: index).pointee
        guard request.kind == UInt32(WHISKER_MEASURE_TEXT) else { continue }
        guard request.font_style <= 2,
              request.wrap <= 1,
              request.word_break <= 2,
              request.overflow <= 1,
              request.direction <= 2,
              request.alignment <= 4,
              request.font_size.isFinite,
              request.font_size > 0,
              request.line_height.isFinite,
              request.line_height >= 0,
              request.letter_spacing.isFinite,
              request.font_optical_sizing <= 1,
              request.font_feature_count <= 4_096,
              request.font_variation_count <= 4_096,
              request.font_feature_count + request.font_variation_count <= 4_096,
              validMeasureFontFeatures(request),
              validMeasureFontVariations(request),
              let families = validatedFontFamilies(request) else { return false }
        textFontFamilies[index] = families
    }
    for index in 0..<count {
        let request = requests.advanced(by: index).pointee
        var response = responses.advanced(by: index).pointee
        response.key = request.key
        response.environment_epoch = request.environment_epoch
        switch request.kind {
        case UInt32(WHISKER_MEASURE_TEXT):
            measureText(
                request,
                fontFamilies: textFontFamilies[index] ?? ["system"],
                response: &response
            )
        case UInt32(WHISKER_MEASURE_REPLACED_CONTENT) where request.intrinsic_mask == 3,
             UInt32(WHISKER_MEASURE_EMBEDDED_SURFACE) where request.intrinsic_mask == 3:
            response.status = UInt32(WHISKER_MEASURE_READY)
            response.width = request.known_mask & 1 != 0
                ? request.known_width : request.intrinsic_width
            response.height = request.known_mask & 2 != 0
                ? request.known_height : request.intrinsic_height
        default:
            measureCustomElement(request, response: &response)
        }
        responses.advanced(by: index).pointee = response
    }
    return true
}

private func measureText(
    _ request: WhiskerMobileMeasureRequest,
    fontFamilies: [String],
    response: inout WhiskerMobileMeasureResponse
) {
    let style: WhiskerTextFontStyle = switch request.font_style {
    case 0: .normal
    case 1: .italic
    default: .oblique
    }
    var baseFont = resolveWhiskerBaseFont(
        fontFamilies: fontFamilies,
        fontSize: CGFloat(request.font_size),
        fontWeight: Int(request.font_weight),
        fontStyle: style
    ).font
    baseFont = configuredMeasureFont(baseFont, request)
    let widthBasis: CGFloat
    if request.known_mask & 1 != 0 {
        widthBasis = CGFloat(request.known_width)
    } else if request.available_width_kind == 0 {
        widthBasis = CGFloat(request.available_width)
    } else {
        widthBasis = 0
    }
    let paragraph = whiskerTextParagraphStyle(request, widthBasis: widthBasis)
    let attributes: [NSAttributedString.Key: Any] = [
        .font: baseFont,
        .kern: CGFloat(request.letter_spacing),
        .paragraphStyle: paragraph,
    ]
    let width = request.available_width_kind == 0 && request.wrap != 0
        ? CGFloat(request.available_width) : CGFloat.greatestFiniteMagnitude
    let source = hostString(request.text)
    let measuredText = request.word_break == 2 ? protectCJKBreaks(source) : source
    var measured = (measuredText as NSString).boundingRect(
        with: CGSize(width: width, height: .greatestFiniteMagnitude),
        options: [.usesLineFragmentOrigin, .usesFontLeading],
        attributes: attributes,
        context: nil
    ).size
    if request.max_lines > 0 {
        let lineHeight = request.line_height > 0
            ? CGFloat(request.line_height) : baseFont.lineHeight
        measured.height = min(measured.height, lineHeight * CGFloat(request.max_lines))
    }
    response.status = UInt32(WHISKER_MEASURE_READY)
    response.width = request.known_mask & 1 != 0
        ? request.known_width : Float(ceil(measured.width))
    response.height = request.known_mask & 2 != 0
        ? request.known_height : Float(ceil(measured.height))
    response.first_baseline = Float(baseFont.ascender)
    response.last_baseline = max(
        response.first_baseline,
        response.height - Float(abs(baseFont.descender))
    )
    response.metrics_mask = 3
}

func whiskerTextParagraphStyle(
    _ request: WhiskerMobileMeasureRequest,
    widthBasis: CGFloat
) -> NSMutableParagraphStyle {
    let paragraph = NSMutableParagraphStyle()
    paragraph.firstLineHeadIndent = CGFloat(request.indent_logical_pixels)
        + widthBasis * CGFloat(request.indent_percentage) / 100
    paragraph.baseWritingDirection = switch request.direction {
    case 0: .natural
    case 1: .leftToRight
    default: .rightToLeft
    }
    paragraph.alignment = switch request.alignment {
    case 0: request.direction == 2 ? .right : .left
    case 1: request.direction == 2 ? .left : .right
    case 2: .left
    case 3: .right
    default: .center
    }
    paragraph.lineBreakMode = request.overflow != 0
        ? .byTruncatingTail
        : (request.word_break == 1 ? .byCharWrapping : .byWordWrapping)
    if request.line_height > 0 {
        paragraph.minimumLineHeight = CGFloat(request.line_height)
        paragraph.maximumLineHeight = CGFloat(request.line_height)
    }
    return paragraph
}

private func validatedFontFamilies(_ request: WhiskerMobileMeasureRequest) -> [String]? {
    guard request.font_family_count > 0,
          request.font_family_count <= 4_096,
          let pointer = request.font_families else { return nil }
    var result = [String]()
    result.reserveCapacity(request.font_family_count)
    for reference in UnsafeBufferPointer(start: pointer, count: request.font_family_count) {
        guard reference.len > 0,
              reference.len <= 1_048_576,
              let bytes = reference.ptr,
              let family = String(
                  bytes: UnsafeBufferPointer(
                      start: UnsafeRawPointer(bytes).assumingMemoryBound(to: UInt8.self),
                      count: reference.len
                  ),
                  encoding: .utf8
              ),
              !family.isEmpty else { return nil }
        result.append(family)
    }
    return result
}

private func validMeasureFontFeatures(_ request: WhiskerMobileMeasureRequest) -> Bool {
    guard (request.font_features == nil) == (request.font_feature_count == 0) else {
        return false
    }
    guard let pointer = request.font_features else { return true }
    return UnsafeBufferPointer(start: pointer, count: request.font_feature_count).allSatisfy {
        validOpenTypeTag($0.tag)
    }
}

private func validMeasureFontVariations(_ request: WhiskerMobileMeasureRequest) -> Bool {
    guard (request.font_variations == nil) == (request.font_variation_count == 0) else {
        return false
    }
    guard let pointer = request.font_variations else { return true }
    return UnsafeBufferPointer(start: pointer, count: request.font_variation_count).allSatisfy {
        validOpenTypeTag($0.tag) && $0.value.isFinite
    }
}

private func validOpenTypeTag<T>(_ tag: T) -> Bool {
    withUnsafeBytes(of: tag) { bytes in
        bytes.count == 4 && bytes.allSatisfy { (0x20...0x7E).contains($0) }
    }
}

private func configuredMeasureFont(
    _ base: UIFont,
    _ request: WhiskerMobileMeasureRequest
) -> UIFont {
    var font = base as CTFont
    var attributes: [CFString: Any] = [:]
    if let pointer = request.font_features, request.font_feature_count > 0 {
        let settings: [[CFString: Any]] = UnsafeBufferPointer(
            start: pointer,
            count: request.font_feature_count
        ).map {
            [kCTFontOpenTypeFeatureTag: openTypeCode($0.tag), kCTFontOpenTypeFeatureValue: $0.value]
        }
        attributes[kCTFontFeatureSettingsAttribute] = settings
    }
    var variations: [NSNumber: NSNumber] = [:]
    if let pointer = request.font_variations, request.font_variation_count > 0 {
        for variation in UnsafeBufferPointer(start: pointer, count: request.font_variation_count) {
            variations[NSNumber(value: openTypeCode(variation.tag))] = NSNumber(value: variation.value)
        }
    }
    let opticalTag = openTypeCode("opsz")
    if request.font_optical_sizing == 0,
       variations[NSNumber(value: opticalTag)] == nil {
        variations[NSNumber(value: opticalTag)] = NSNumber(value: request.font_size)
    }
    if !variations.isEmpty {
        attributes[kCTFontVariationAttribute] = variations
    }
    if !attributes.isEmpty {
        let descriptor = CTFontDescriptorCreateWithAttributes(attributes as CFDictionary)
        font = CTFontCreateCopyWithAttributes(font, CGFloat(request.font_size), nil, descriptor)
    }
    return font as UIFont
}

private func openTypeCode<T>(_ tag: T) -> UInt32 {
    withUnsafeBytes(of: tag) { bytes in
        bytes.reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
    }
}

private func openTypeCode(_ tag: String) -> UInt32 {
    tag.utf8.reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
}

private func protectCJKBreaks(_ value: String) -> String {
    var result = ""
    var previousWasCJK = false
    for character in value {
        let currentIsCJK = character.unicodeScalars.contains { scalar in
            (0x2E80...0x9FFF).contains(scalar.value)
                || (0xF900...0xFAFF).contains(scalar.value)
                || (0xAC00...0xD7AF).contains(scalar.value)
        }
        if previousWasCJK && currentIsCJK { result.append("\u{2060}") }
        result.append(character)
        previousWasCJK = currentIsCJK
    }
    return result
}

private func measureCustomElement(
    _ request: WhiskerMobileMeasureRequest,
    response: inout WhiskerMobileMeasureResponse
) {
    let payload = request.payload.ptr.map {
        Data(bytes: $0, count: request.payload.len)
    } ?? Data()
    let custom = WhiskerElementRegistry.measure(
        Int(request.element_type),
        request: WhiskerMeasureRequest(
            availableWidth: request.available_width_kind == 0
                ? CGFloat(request.available_width) : nil,
            availableHeight: request.available_height_kind == 0
                ? CGFloat(request.available_height) : nil,
            knownWidth: request.known_mask & 1 != 0 ? CGFloat(request.known_width) : nil,
            knownHeight: request.known_mask & 2 != 0 ? CGFloat(request.known_height) : nil,
            payloadVersion: request.payload_version,
            payload: payload
        )
    )
    if let custom {
        response.status = UInt32(WHISKER_MEASURE_READY)
        response.width = request.known_mask & 1 != 0
            ? request.known_width : Float(custom.width)
        response.height = request.known_mask & 2 != 0
            ? request.known_height : Float(custom.height)
    } else {
        response.status = UInt32(WHISKER_MEASURE_UNSUPPORTED)
        response.reason = 1
    }
}
