pub(super) fn app_path(base: &str, pathname: &str) -> Result<String, String> {
    let prefix = base.trim_end_matches('/');
    match pathname.strip_prefix(prefix) {
        Some("") => Ok("/".into()),
        Some(path) if path.starts_with('/') => Ok(path.into()),
        _ => Err("browser location is outside the application base path".into()),
    }
}

pub(super) fn browser_url(base: &str, app_url: &str) -> Result<String, String> {
    if !app_url.starts_with('/') || app_url.starts_with("//") {
        return Err("router URL must be an absolute application path".into());
    }
    Ok(format!("{}{app_url}", base.trim_end_matches('/')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subpath_navigation_preserves_queries_and_fragments() {
        let base = "/examples/chat/";
        assert_eq!(app_path(base, "/examples/chat").unwrap(), "/");
        assert_eq!(app_path(base, "/examples/chat/").unwrap(), "/");
        assert_eq!(
            app_path(base, "/examples/chat/settings").unwrap(),
            "/settings"
        );
        assert_eq!(
            browser_url(base, "/settings?q=rust#key").unwrap(),
            "/examples/chat/settings?q=rust#key"
        );
        assert_eq!(browser_url(base, "/").unwrap(), "/examples/chat/");
        assert!(app_path(base, "/examples/chat-other").is_err());
        assert!(app_path(base, "/settings").is_err());
        assert!(browser_url(base, "//other.test/").is_err());
    }

    #[test]
    fn root_deployment_keeps_existing_urls() {
        assert_eq!(app_path("/", "/settings").unwrap(), "/settings");
        assert_eq!(browser_url("/", "/settings?q=x").unwrap(), "/settings?q=x");
    }
}
