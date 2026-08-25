import UIKit
import WhiskerModule

let whiskerIOSMeasure: WhiskerMeasureHost = { data, requests, count, responses in
    guard data != nil, let requests, let responses else { return false }
    for index in 0..<count {
        let request = requests.advanced(by: index).pointee
        var response = responses.advanced(by: index).pointee
        response.key = request.key
        response.environment_epoch = request.environment_epoch
        switch request.kind {
        case UInt32(WHISKER_MEASURE_TEXT):
            measureText(request, response: &response)
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
    response: inout WhiskerMobileMeasureResponse
) {
    let family = hostString(request.font_family)
    let weightValue = max(-1, min(1, CGFloat(Int(request.font_weight) - 400) / 500))
    var baseFont = family.isEmpty
        ? UIFont.systemFont(
            ofSize: CGFloat(request.font_size),
            weight: UIFont.Weight(rawValue: weightValue)
        )
        : UIFont(name: family, size: CGFloat(request.font_size))
            ?? UIFont.systemFont(ofSize: CGFloat(request.font_size))
    if request.font_style != 0,
       let descriptor = baseFont.fontDescriptor.withSymbolicTraits(
           baseFont.fontDescriptor.symbolicTraits.union(.traitItalic)
       ) {
        baseFont = UIFont(descriptor: descriptor, size: CGFloat(request.font_size))
    }
    let paragraph = NSMutableParagraphStyle()
    let widthBasis: CGFloat
    if request.known_mask & 1 != 0 {
        widthBasis = CGFloat(request.known_width)
    } else if request.available_width_kind == 0 {
        widthBasis = CGFloat(request.available_width)
    } else {
        widthBasis = 0
    }
    paragraph.firstLineHeadIndent = CGFloat(request.indent_logical_pixels)
        + widthBasis * CGFloat(request.indent_percentage) / 100
    paragraph.lineBreakMode = request.overflow != 0
        ? .byTruncatingTail
        : (request.word_break == 1 ? .byCharWrapping : .byWordWrapping)
    if request.line_height > 0 {
        paragraph.minimumLineHeight = CGFloat(request.line_height)
        paragraph.maximumLineHeight = CGFloat(request.line_height)
    }
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
