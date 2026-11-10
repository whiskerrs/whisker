#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif
import WhiskerModule

func parsePaintColor(_ value: WhiskerMobileColor) -> WhiskerPlatformColor {
    if value.kind == 1 {
        return WhiskerPlatformColor(
            red: CGFloat(value.red) / 255,
            green: CGFloat(value.green) / 255,
            blue: CGFloat(value.blue) / 255,
            alpha: CGFloat(value.alpha)
        )
    }
    switch hostString(value.name).lowercased() {
    case "black": return .black
    case "white": return .white
    case "red": return .red
    // UIKit's `.green` is #00ff00; CSS named `green` is #008000.
    case "green": return WhiskerPlatformColor(red: 0, green: 128 / 255, blue: 0, alpha: 1)
    case "gold": return WhiskerPlatformColor(red: 1, green: 215 / 255, blue: 0, alpha: 1)
    case "blue": return .blue
    default: return .clear
    }
}
