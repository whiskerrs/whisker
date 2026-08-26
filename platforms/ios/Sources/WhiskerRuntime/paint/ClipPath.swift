import UIKit
import WhiskerModule

enum HostClipReferenceBox {
    case border
    case padding
    case content
}

/// Rounded inset basic shape copied out of the borrowed mobile ABI payload.
struct HostInsetClipPath {
    let referenceBox: HostClipReferenceBox
    let edges: [WhiskerMobileLengthPercentage]
    let radiiHorizontal: [WhiskerMobileLengthPercentage]
    let radiiVertical: [WhiskerMobileLengthPercentage]

    func path(in bounds: CGRect, contentBox: CGRect, painter: HostBoxPainter) -> CGPath {
        let reference = switch referenceBox {
        case .border: bounds
        case .padding: painter.paddingBoxRect(in: bounds)
        case .content: contentBox
        }
        let top = resolve(edges[0], axis: reference.height)
        let right = resolve(edges[1], axis: reference.width)
        let bottom = resolve(edges[2], axis: reference.height)
        let left = resolve(edges[3], axis: reference.width)
        let inset = CGRect(
            x: reference.minX + left,
            y: reference.minY + top,
            width: max(0, reference.width - left - right),
            height: max(0, reference.height - top - bottom)
        )
        let radii = radiiHorizontal.indices.map { index in
            CGSize(
                width: max(0, resolve(radiiHorizontal[index], axis: inset.width)),
                height: max(0, resolve(radiiVertical[index], axis: inset.height))
            )
        }
        return roundedPath(in: inset, radii: radii).cgPath
    }
}

private func resolve(_ value: WhiskerMobileLengthPercentage, axis: CGFloat) -> CGFloat {
    CGFloat(value.length) + CGFloat(value.fraction) * axis
}
