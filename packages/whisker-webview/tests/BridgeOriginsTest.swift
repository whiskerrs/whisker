import Foundation
@main struct BridgeOriginsTest {
    static func main() {
        let allowed: Set<String> = ["https://example.com:443"]
        precondition(BridgeOrigins.permits("https://EXAMPLE.com", true, allowed))
        for value in ["https://evil.test", "http://example.com", "https://example.com:444", "about:blank", "https://*.example.com", "https://user@example.com", "https://example.com/path"] {
            precondition(!BridgeOrigins.permits(value, true, allowed), value)
        }
        precondition(!BridgeOrigins.permits("https://example.com", false, allowed))
        precondition(!BridgeOrigins.permits("https://example.com", true, []))
        print("bridge origin tests passed")
    }
}
