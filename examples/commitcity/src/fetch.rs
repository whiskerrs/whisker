//! Fetching a GitHub user's public contribution calendar.

use crate::data::{self, Year};

/// Users with a bundled snapshot, shown as presets and used when whisker.rs is unreachable.
pub const PRESETS: [(&str, &str); 5] = [
    ("itome", include_str!("../assets/snapshots/itome.json")),
    (
        "torvalds",
        include_str!("../assets/snapshots/torvalds.json"),
    ),
    ("dtolnay", include_str!("../assets/snapshots/dtolnay.json")),
    (
        "sindresorhus",
        include_str!("../assets/snapshots/sindresorhus.json"),
    ),
    (
        "yyx990803",
        include_str!("../assets/snapshots/yyx990803.json"),
    ),
];

const API: &str = "https://whisker.rs/api/github/contributions/";

pub fn snapshot(user: &str) -> Option<Year> {
    let (_, json) = PRESETS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(user))?;
    data::parse(json).ok()
}

/// The login part of `input`, which may also be a profile URL or start with `@`.
pub fn login(input: &str) -> Result<String, String> {
    let input = input.trim().trim_end_matches('/');
    let login = input
        .rsplit_once('/')
        .map_or(input, |(_, last)| last)
        .trim_start_matches('@');
    let valid = (1..=39).contains(&login.len())
        && !login.starts_with('-')
        && login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if valid {
        Ok(login.to_string())
    } else {
        Err("Enter a GitHub username".into())
    }
}

pub async fn year(input: String) -> Result<Year, String> {
    let user = login(&input)?;
    match get(format!("{API}{user}")).await {
        Ok(body) => data::parse(&body),
        Err(error) => snapshot(&user).ok_or(error),
    }
}

fn status_error(status: u16) -> String {
    match status {
        404 => "No GitHub user by that name".into(),
        _ => format!("Couldn't load contributions (HTTP {status})"),
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn get(url: String) -> Result<String, String> {
    whisker::run_blocking(move || {
        match ureq::get(&url)
            .set("User-Agent", "whisker-commitcity")
            .timeout(std::time::Duration::from_secs(20))
            .call()
        {
            Ok(response) => response.into_string().map_err(|e| e.to_string()),
            Err(ureq::Error::Status(status, _)) => Err(status_error(status)),
            Err(e) => Err(e.to_string()),
        }
    })
    .await
}

#[cfg(target_arch = "wasm32")]
async fn get(url: String) -> Result<String, String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    let error = |e: wasm_bindgen::JsValue| format!("{e:?}");
    let window = web_sys::window().ok_or("no window")?;
    let response: web_sys::Response = JsFuture::from(window.fetch_with_str(&url))
        .await
        .map_err(error)?
        .unchecked_into();
    if !response.ok() {
        return Err(status_error(response.status()));
    }
    let text = JsFuture::from(response.text().map_err(error)?)
        .await
        .map_err(error)?;
    Ok(text.as_string().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::login;

    #[test]
    fn logins_come_from_names_handles_and_profile_urls() {
        assert_eq!(login(" itome ").unwrap(), "itome");
        assert_eq!(login("@itome").unwrap(), "itome");
        assert_eq!(login("https://github.com/itome/").unwrap(), "itome");
        assert!(login("").is_err());
        assert!(login("-itome").is_err());
        assert!(login("it ome").is_err());
    }
}
