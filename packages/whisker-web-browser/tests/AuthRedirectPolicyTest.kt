package rs.whisker.modules.webbrowser
fun main() {
    val redirect = "myapp://auth/callback"
    check(AuthRedirectPolicy.matches(redirect, "$redirect?code=ok&state=nonce"))
    for (url in listOf("$redirect.evil?code=bad", "myapp://auth.evil/callback", "myapp://user@auth/callback", "myapp://auth/callback/extra", "other://auth/callback", "myapp://auth:99/callback")) {
        check(!AuthRedirectPolicy.matches(redirect, url)) { url }
    }
    check(AuthRedirectPolicy.matches("https://example.com/cb", "https://EXAMPLE.com:443/cb?code=ok"))
    check(!AuthRedirectPolicy.matches("https://example.com/cb?fixed=1", "https://example.com/cb?fixed=2&code=bad"))
    check(!AuthRedirectPolicy.matches("https://example.com/cb?fixed=1", "https://example.com/cb?fixed=1&fixed=2"))
    check(!AuthRedirectPolicy.matches("myapp://auth/callback#fixed", "myapp://auth/callback#fixed"))
    println("OAuth redirect regression cases passed")
}
