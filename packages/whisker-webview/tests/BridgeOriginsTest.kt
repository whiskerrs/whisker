package rs.whisker.elements.webview
fun main() {
    val allowed = setOf("https://example.com:443")
    check(BridgeOrigins.permits("https://EXAMPLE.com", true, allowed))
    for (value in listOf("https://evil.test", "http://example.com", "https://example.com:444", "about:blank", "https://*.example.com", "https://user@example.com", "https://example.com/path")) {
        check(!BridgeOrigins.permits(value, true, allowed)) { value }
    }
    check(!BridgeOrigins.permits("https://example.com", false, allowed))
    check(!BridgeOrigins.permits("https://example.com", true, emptySet()))
    println("bridge origin tests passed")
}
