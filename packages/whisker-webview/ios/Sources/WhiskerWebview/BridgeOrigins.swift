import Foundation

enum BridgeOrigins {
    static func canonical(_ value: String) -> String? {
        guard value.unicodeScalars.allSatisfy({ $0.value > 32 && $0.value < 127 && $0.value != 92 && $0.value != 42 }),
              let url = URLComponents(string: value),
              let scheme = url.scheme?.lowercased(), ["http", "https"].contains(scheme),
              let host = url.host?.lowercased(), !host.isEmpty, !host.hasSuffix("."),
              url.user == nil, url.password == nil, url.query == nil, url.fragment == nil,
              ["", "/"].contains(url.path), url.port == nil || (0...65535).contains(url.port!) else { return nil }
        return "\(scheme)://\(host):\(url.port ?? (scheme == "https" ? 443 : 80))"
    }

    static func permits(_ value: String, _ mainFrame: Bool, _ origins: Set<String>) -> Bool {
        guard mainFrame, let origin = canonical(value) else { return false }
        return origins.contains(origin)
    }
}
