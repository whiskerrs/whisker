import UIKit
import WhiskerModule

internal struct InputTypography {
  let font: UIFont
  let lineHeight: CGFloat?
  let letterSpacing: CGFloat

  init(
    family: String?, size: CGFloat, weight: Int, italic: Bool, lineHeight: CGFloat?,
    letterSpacing: CGFloat
  ) {
    let base =
      family.flatMap { UIFont(name: $0, size: size) }
      ?? UIFont.systemFont(ofSize: size, weight: Self.weight(weight))
    let descriptor = italic ? base.fontDescriptor.withSymbolicTraits(.traitItalic) : nil
    self.font = descriptor.map { UIFont(descriptor: $0, size: size) } ?? base
    self.lineHeight = lineHeight
    self.letterSpacing = letterSpacing
  }

  init(_ style: WhiskerTextStyle) {
    self.init(
      family: style.fontFamilies.first.flatMap { $0 == "system" ? nil : $0 }, size: style.fontSize,
      weight: style.fontWeight, italic: style.fontStyle != .normal, lineHeight: style.lineHeight,
      letterSpacing: style.letterSpacing)
  }

  func attributes(alignment: NSTextAlignment = .natural) -> [NSAttributedString.Key: Any] {
    let paragraph = NSMutableParagraphStyle()
    paragraph.alignment = alignment
    paragraph.lineBreakMode = .byWordWrapping
    if let lineHeight {
      paragraph.minimumLineHeight = lineHeight
      paragraph.maximumLineHeight = lineHeight
    }
    return [.font: font, .kern: letterSpacing, .paragraphStyle: paragraph]
  }

  private static func weight(_ value: Int) -> UIFont.Weight {
    switch value {
    case ...150: .ultraLight
    case ...250: .thin
    case ...350: .light
    case ...450: .regular
    case ...550: .medium
    case ...650: .semibold
    case ...750: .bold
    case ...850: .heavy
    default: .black
    }
  }
}

private struct InputMeasureData {
  let text: String
  let multiline: Bool
  let fontFamily: String?
  let fontSize: CGFloat
  let fontWeight: Int
  let italic: Bool
  let lineHeight: CGFloat?
  let letterSpacing: CGFloat

  init?(_ payload: WhiskerValue) {
    guard case .map(let fields) = payload,
      let text = fields["text"]?.asString,
      let multiline = fields["multiline"]?.asBool,
      let fontSize = fields["font_size"]?.asDouble,
      let fontWeight = fields["font_weight"]?.asInt,
      let italic = fields["italic"]?.asBool,
      let letterSpacing = fields["letter_spacing"]?.asDouble
    else { return nil }
    self.text = text
    self.multiline = multiline
    self.fontFamily = fields["font_family"]?.asString
    self.fontSize = CGFloat(fontSize)
    self.fontWeight = Int(fontWeight)
    self.italic = italic
    self.lineHeight = fields["line_height"]?.asDouble.map { CGFloat($0) }
    self.letterSpacing = CGFloat(letterSpacing)
  }
}

internal func measureInput(_ request: WhiskerMeasureRequest) -> WhiskerMeasuredSize? {
  guard request.payloadVersion == 1, let input = InputMeasureData(request.payload) else { return nil }
  let typography = InputTypography(
    family: input.fontFamily, size: input.fontSize, weight: input.fontWeight, italic: input.italic,
    lineHeight: input.lineHeight, letterSpacing: input.letterSpacing)
  let text = input.multiline ? input.text : input.text.replacingOccurrences(of: "\n", with: " ")
  let attributes = typography.attributes()
  let natural = (text as NSString).boundingRect(
    with: CGSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude),
    options: [.usesLineFragmentOrigin, .usesFontLeading], attributes: attributes, context: nil
  ).width
  let available: CGFloat
  switch request.availableWidthKind {
  case .definite: available = min(natural, request.availableWidth ?? natural)
  case .minContent: available = input.multiline ? 1 : natural
  case .maxContent: available = natural
  }
  let width = max(1, request.knownWidth ?? available)
  let storage = NSTextStorage(string: text, attributes: attributes)
  let layout = NSLayoutManager()
  let container = NSTextContainer(
    size: CGSize(width: width, height: CGFloat.greatestFiniteMagnitude))
  container.lineFragmentPadding = 0
  container.maximumNumberOfLines = input.multiline ? 0 : 1
  layout.addTextContainer(container)
  storage.addLayoutManager(layout)
  layout.ensureLayout(for: container)
  let used = layout.usedRect(for: container)
  let height = max(
    typography.lineHeight ?? typography.font.lineHeight,
    max(used.maxY, layout.extraLineFragmentRect.maxY))
  return WhiskerMeasuredSize(
    width: request.knownWidth ?? used.width, height: request.knownHeight ?? height)
}
