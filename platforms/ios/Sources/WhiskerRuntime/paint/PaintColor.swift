import UIKit
import WhiskerModule

func parsePaintColor(_ value: WhiskerMobileColor) -> UIColor {
    if value.kind == 1 {
        return UIColor(
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
    case "green": return .green
    case "gold": return UIColor(red: 1, green: 215 / 255, blue: 0, alpha: 1)
    case "blue": return .blue
    default: return .clear
    }
}
