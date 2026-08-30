import UIKit
import WhiskerModule

func validBorrowedArray<T>(_ pointer: UnsafePointer<T>?, _ count: Int) -> Bool {
    (pointer == nil) == (count == 0)
}

func validBorrowedStrings(
    _ pointer: UnsafePointer<WhiskerStringRef>?,
    _ count: Int
) -> Bool {
    guard let pointer, count > 0 else { return false }
    return UnsafeBufferPointer(start: pointer, count: count).allSatisfy {
        $0.len > 0 && $0.ptr != nil
    }
}

func hostFontFeatures(_ content: WhiskerMobileText) -> [WhiskerFontFeature] {
    guard let pointer = content.font_features else { return [] }
    return UnsafeBufferPointer(start: pointer, count: content.font_feature_count).map {
        WhiskerFontFeature(tag: hostFontTag($0.tag), value: $0.value)
    }
}

func hostFontFamilies(_ content: WhiskerMobileText) -> [String] {
    guard let pointer = content.font_families else { return [] }
    return UnsafeBufferPointer(start: pointer, count: content.font_family_count).map(hostString)
}

func hostFontVariations(_ content: WhiskerMobileText) -> [WhiskerFontVariation] {
    guard let pointer = content.font_variations else { return [] }
    return UnsafeBufferPointer(start: pointer, count: content.font_variation_count).map {
        WhiskerFontVariation(tag: hostFontTag($0.tag), value: CGFloat($0.value))
    }
}

func hostFontTag<T>(_ value: T) -> String {
    withUnsafeBytes(of: value) { String(decoding: $0, as: UTF8.self) }
}

func hostRect(_ value: WhiskerMobileRect) -> CGRect {
    CGRect(
        x: CGFloat(value.x),
        y: CGFloat(value.y),
        width: CGFloat(value.width),
        height: CGFloat(value.height)
    )
}

func validGradientStops(
    _ payload: UnsafeRawPointer,
    count: Int,
    requiresFractionOnly: Bool = false
) -> Bool {
    guard (2...4_096).contains(count) else { return false }
    let stops = UnsafeBufferPointer(
        start: payload.assumingMemoryBound(to: WhiskerMobileGradientStop.self),
        count: count
    )
    return stops.allSatisfy { stop in
        stop.position.isFinite && (!requiresFractionOnly || stop.position.length == 0) &&
            stop.color.kind <= 1 && stop.color.alpha.isFinite &&
            (0...1).contains(stop.color.alpha)
    }
}

func validHardBoxShadow(_ shadow: WhiskerMobileBoxShadow) -> Bool {
    shadow.offset_x.isFinite && shadow.offset_y.isFinite &&
        shadow.blur_radius.isFinite && shadow.blur_radius >= 0 && shadow.spread_radius.isFinite &&
        (shadow.inset == 0 || shadow.inset == 1) &&
        shadow.color.kind <= 1 && shadow.color.alpha.isFinite &&
        (0...1).contains(shadow.color.alpha)
}

func validClipPath(_ clip: WhiskerMobileClipPath) -> Bool {
    guard clip.reference_box <= UInt32(WHISKER_BACKGROUND_BOX_CONTENT),
          clip.payload_count == 1,
          let payload = clip.payload else { return false }
    switch clip.shape_kind {
    case UInt32(WHISKER_CLIP_SHAPE_INSET):
        let inset = payload.assumingMemoryBound(to: WhiskerMobileClipInset.self).pointee
        let radii = tupleArray(inset.radii_horizontal) + tupleArray(inset.radii_vertical)
        return tupleArray(inset.edges).allSatisfy(\.isFinite) && radii.allSatisfy {
            $0.isFinite && $0.length >= 0 && $0.fraction >= 0
        }
    case UInt32(WHISKER_CLIP_SHAPE_CIRCLE):
        let circle = payload.assumingMemoryBound(to: WhiskerMobileClipCircle.self).pointee
        return circle.radius.isNonNegativeFinite && circle.center_x.isFinite && circle.center_y.isFinite
    case UInt32(WHISKER_CLIP_SHAPE_ELLIPSE):
        let ellipse = payload.assumingMemoryBound(to: WhiskerMobileClipEllipse.self).pointee
        return ellipse.radius_x.isNonNegativeFinite && ellipse.radius_y.isNonNegativeFinite &&
            ellipse.center_x.isFinite && ellipse.center_y.isFinite
    case UInt32(WHISKER_CLIP_SHAPE_PATH):
        let path = payload.assumingMemoryBound(to: WhiskerMobileClipPathCommands.self).pointee
        guard path.fill_rule <= UInt32(WHISKER_FILL_RULE_EVEN_ODD),
              path.command_count > 0, path.command_count <= 4096,
              let commands = path.commands else { return false }
        return UnsafeBufferPointer(start: commands, count: path.command_count).allSatisfy {
            $0.kind <= UInt32(WHISKER_PATH_CLOSE) && tupleArray($0.points).allSatisfy(\.isFinite)
        }
    default:
        return false
    }
}

func validBackgroundLayer(
    _ layer: WhiskerMobileBackgroundLayer,
    resources: HostResourceStore
) -> Bool {
    guard validBackgroundGeometry(layer), layer.image.scalar.isFinite else { return false }
    switch layer.image.kind {
    case UInt32(WHISKER_BACKGROUND_RESOURCE):
        guard layer.image.payload_count == 1,
              let resource = layer.image.payload?.assumingMemoryBound(to: UInt64.self).pointee,
              resource != 0 else { return false }
        return resources.rasterImage(id: resource) != nil
    case UInt32(WHISKER_BACKGROUND_LINEAR):
        return layer.image.payload.map {
            validGradientStops($0, count: layer.image.payload_count)
        } ?? false
    case UInt32(WHISKER_BACKGROUND_RADIAL):
        guard layer.image.payload_count == 1,
              let radial = layer.image.payload?.assumingMemoryBound(
                  to: WhiskerMobileRadialGradient.self
              ).pointee,
              radial.center_x.isFinite, radial.center_y.isFinite,
              radial.radius_x.isFinite, radial.radius_y.isFinite,
              (2...4_096).contains(radial.stop_count),
              let stops = radial.stops else { return false }
        return validGradientStops(UnsafeRawPointer(stops), count: radial.stop_count)
    case UInt32(WHISKER_BACKGROUND_CONIC):
        guard layer.image.payload_count == 1,
              let conic = layer.image.payload?.assumingMemoryBound(
                  to: WhiskerMobileConicGradient.self
              ).pointee,
              conic.center_x.isFinite, conic.center_y.isFinite,
              (2...4_096).contains(conic.stop_count),
              let stops = conic.stops else { return false }
        return validGradientStops(
            UnsafeRawPointer(stops),
            count: conic.stop_count,
            requiresFractionOnly: true
        )
    default:
        return false
    }
}

func validBackgroundGeometry(_ layer: WhiskerMobileBackgroundLayer) -> Bool {
    guard layer.position_x.isFinite, layer.position_y.isFinite,
          layer.attachment == UInt32(WHISKER_BACKGROUND_ATTACHMENT_SCROLL),
          layer.blend_mode == UInt32(WHISKER_BACKGROUND_BLEND_NORMAL) else {
        return false
    }
    let supportedRepeats = [
        UInt32(WHISKER_BACKGROUND_REPEAT),
        UInt32(WHISKER_BACKGROUND_NO_REPEAT),
        UInt32(WHISKER_BACKGROUND_SPACE),
        UInt32(WHISKER_BACKGROUND_ROUND)
    ]
    let validSize = switch layer.size_kind {
    case UInt32(WHISKER_BACKGROUND_SIZE_AUTO),
         UInt32(WHISKER_BACKGROUND_SIZE_COVER),
         UInt32(WHISKER_BACKGROUND_SIZE_CONTAIN):
        layer.size_width.isZero && layer.size_height.isZero
    case UInt32(WHISKER_BACKGROUND_SIZE_EXPLICIT):
        layer.size_width.isNonNegativeFinite && layer.size_height.isNonNegativeFinite
    case UInt32(WHISKER_BACKGROUND_SIZE_WIDTH):
        layer.size_width.isNonNegativeFinite && layer.size_height.isZero
    case UInt32(WHISKER_BACKGROUND_SIZE_HEIGHT):
        layer.size_width.isZero && layer.size_height.isNonNegativeFinite
    default:
        false
    }
    return validSize && supportedRepeats.contains(layer.repeat_x) &&
        supportedRepeats.contains(layer.repeat_y) &&
        [UInt32(WHISKER_BACKGROUND_BOX_BORDER), UInt32(WHISKER_BACKGROUND_BOX_PADDING),
         UInt32(WHISKER_BACKGROUND_BOX_CONTENT)]
            .contains(layer.origin) &&
        [UInt32(WHISKER_BACKGROUND_BOX_BORDER), UInt32(WHISKER_BACKGROUND_BOX_PADDING),
         UInt32(WHISKER_BACKGROUND_BOX_CONTENT), UInt32(WHISKER_BACKGROUND_BOX_BORDER_AREA)]
            .contains(layer.clip)
}

func hostBackgroundGeometry(
    _ layer: WhiskerMobileBackgroundLayer,
    intrinsicSize: CGSize?
) -> HostBackgroundGeometry? {
    guard validBackgroundGeometry(layer) else { return nil }
    let sizeKind: HostBackgroundSize
    let sizeWidth: WhiskerMobileLengthPercentage?
    let sizeHeight: WhiskerMobileLengthPercentage?
    switch layer.size_kind {
    case UInt32(WHISKER_BACKGROUND_SIZE_AUTO):
        sizeKind = .auto
        sizeWidth = nil
        sizeHeight = nil
    case UInt32(WHISKER_BACKGROUND_SIZE_EXPLICIT):
        sizeKind = .explicit
        sizeWidth = layer.size_width
        sizeHeight = layer.size_height
    case UInt32(WHISKER_BACKGROUND_SIZE_COVER):
        sizeKind = .cover
        sizeWidth = nil
        sizeHeight = nil
    case UInt32(WHISKER_BACKGROUND_SIZE_CONTAIN):
        sizeKind = .contain
        sizeWidth = nil
        sizeHeight = nil
    case UInt32(WHISKER_BACKGROUND_SIZE_WIDTH):
        sizeKind = .explicit
        sizeWidth = layer.size_width
        sizeHeight = nil
    case UInt32(WHISKER_BACKGROUND_SIZE_HEIGHT):
        sizeKind = .explicit
        sizeWidth = nil
        sizeHeight = layer.size_height
    default:
        return nil
    }
    return HostBackgroundGeometry(
        positionX: layer.position_x,
        positionY: layer.position_y,
        sizeWidth: sizeWidth,
        sizeHeight: sizeHeight,
        repeatX: hostBackgroundRepeat(layer.repeat_x),
        repeatY: hostBackgroundRepeat(layer.repeat_y),
        origin: hostBackgroundBox(layer.origin),
        clip: hostBackgroundBox(layer.clip),
        sizeKind: sizeKind,
        intrinsicSize: intrinsicSize
    )
}

func hostBackgroundLayer(
    _ layer: WhiskerMobileBackgroundLayer,
    resources: HostResourceStore
) -> HostBackgroundLayer? {
    let image: HostBackgroundImage
    let intrinsicSize: CGSize?
    switch layer.image.kind {
    case UInt32(WHISKER_BACKGROUND_RESOURCE):
        guard layer.image.payload_count == 1,
              let resource = layer.image.payload?.assumingMemoryBound(to: UInt64.self).pointee,
              let raster = resources.rasterResource(id: resource) else { return nil }
        image = .raster(raster.image)
        intrinsicSize = raster.intrinsicSize
    case UInt32(WHISKER_BACKGROUND_LINEAR):
        guard let payload = layer.image.payload else { return nil }
        image = .linear(HostLinearGradient(
            angleDegrees: CGFloat(layer.image.scalar),
            stops: UnsafeBufferPointer(
                start: payload.assumingMemoryBound(to: WhiskerMobileGradientStop.self),
                count: layer.image.payload_count
            ).map(HostLinearGradientStop.init)
        ))
        intrinsicSize = nil
    case UInt32(WHISKER_BACKGROUND_RADIAL):
        guard let radial = layer.image.payload?.assumingMemoryBound(
            to: WhiskerMobileRadialGradient.self
        ).pointee, let stopPointer = radial.stops else { return nil }
        image = .radial(HostRadialGradient(
            centerX: radial.center_x,
            centerY: radial.center_y,
            radiusX: radial.radius_x,
            radiusY: radial.radius_y,
            stops: UnsafeBufferPointer(
                start: stopPointer,
                count: radial.stop_count
            ).map(HostLinearGradientStop.init)
        ))
        intrinsicSize = nil
    case UInt32(WHISKER_BACKGROUND_CONIC):
        guard let conic = layer.image.payload?.assumingMemoryBound(
            to: WhiskerMobileConicGradient.self
        ).pointee, let stopPointer = conic.stops else { return nil }
        image = .conic(HostConicGradient(
            fromDegrees: CGFloat(layer.image.scalar),
            centerX: conic.center_x,
            centerY: conic.center_y,
            stops: UnsafeBufferPointer(
                start: stopPointer,
                count: conic.stop_count
            ).map(HostLinearGradientStop.init)
        ))
        intrinsicSize = nil
    default:
        return nil
    }
    guard let geometry = hostBackgroundGeometry(layer, intrinsicSize: intrinsicSize) else {
        return nil
    }
    return HostBackgroundLayer(image: image, geometry: geometry)
}

func hostBackgroundRepeat(_ value: UInt32) -> HostBackgroundRepeat {
    switch value {
    case UInt32(WHISKER_BACKGROUND_NO_REPEAT): .noRepeat
    case UInt32(WHISKER_BACKGROUND_SPACE): .space
    case UInt32(WHISKER_BACKGROUND_ROUND): .round
    default: .repeat
    }
}

func hostBackgroundBox(_ value: UInt32) -> HostBackgroundBox {
    switch value {
    case UInt32(WHISKER_BACKGROUND_BOX_BORDER): .border
    case UInt32(WHISKER_BACKGROUND_BOX_CONTENT): .content
    case UInt32(WHISKER_BACKGROUND_BOX_BORDER_AREA): .borderArea
    default: .padding
    }
}

private extension WhiskerMobileLengthPercentage {
    var isFinite: Bool { length.isFinite && fraction.isFinite }
    var isZero: Bool { length == 0 && fraction == 0 }
    var isNonNegativeFinite: Bool {
        isFinite && length >= 0 && fraction >= 0
    }
}
