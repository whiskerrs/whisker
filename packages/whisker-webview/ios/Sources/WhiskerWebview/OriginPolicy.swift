import Foundation

enum OriginPolicy {
    private static func parse(_ value: String) -> URLComponents? {
        guard value.unicodeScalars.allSatisfy({ $0.value > 32 && $0.value < 127 && $0.value != 92 }),
              let url = URLComponents(string: value),
              let scheme = url.scheme?.lowercased(), ["http", "https"].contains(scheme),
              let host = url.host, !host.isEmpty, !host.hasSuffix("."),
              url.user == nil, url.password == nil,
              url.port == nil || (0...65535).contains(url.port!) else { return nil }
        return url
    }

    private static func port(_ url: URLComponents) -> Int {
        url.port ?? (url.scheme?.lowercased() == "https" ? 443 : 80)
    }

    static func matches(_ pattern: String, _ value: String) -> Bool {
        guard let candidate = parse(value) else { return false }
        let wildcard = pattern.contains("://*")
        let expanded = pattern.replacingOccurrences(of: "://*.", with: "://whisker-wildcard.")
            .replacingOccurrences(of: "://*", with: "://whisker-any")
        guard !expanded.contains("*"), let rule = parse(expanded),
              ["", "/"].contains(rule.path), rule.query == nil, rule.fragment == nil,
              candidate.scheme?.lowercased() == rule.scheme?.lowercased(),
              port(candidate) == port(rule),
              let host = candidate.host?.lowercased(), let allowed = rule.host?.lowercased() else { return false }
        if wildcard && allowed == "whisker-any" { return true }
        if wildcard && allowed.hasPrefix("whisker-wildcard.") {
            let suffix = String(allowed.dropFirst("whisker-wildcard".count))
            return host.hasSuffix(suffix) && host.count > suffix.count
        }
        return !wildcard && host == allowed
    }
}
