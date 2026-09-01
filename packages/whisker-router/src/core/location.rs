//! Public locations and internal qualified destinations.
//!
//! A route-group segment such as `(home)` selects a navigator branch but is
//! not part of the public URL.  [`Location`] therefore stores only the URL a
//! host exposes, while the internal `Destination` retains the qualified pathname used to
//! resolve a particular placement in the route tree.

/// The public, host-visible location of a routed screen.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Location {
    /// The concrete public pathname. Route-group segments are excluded.
    pub pathname: String,
    /// The query string without the leading `?`.
    pub search: Option<String>,
    /// The fragment without the leading `#`.
    pub fragment: Option<String>,
}

impl Location {
    /// The root location.
    pub fn root() -> Self {
        Self {
            pathname: "/".to_string(),
            search: None,
            fragment: None,
        }
    }

    /// Reconstruct the public URL.
    pub fn to_url(&self) -> String {
        let mut out = self.pathname.clone();
        if let Some(search) = &self.search {
            out.push('?');
            out.push_str(search);
        }
        if let Some(fragment) = &self.fragment {
            out.push('#');
            out.push_str(fragment);
        }
        out
    }
}

impl Default for Location {
    fn default() -> Self {
        Self::root()
    }
}

/// A parsed navigation target.
///
/// The qualified pathname may contain group segments (`/(search)/post/42`),
/// while `location.pathname` is always the public form (`/post/42`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Destination {
    pub qualified_pathname: String,
    pub location: Location,
}

impl Destination {
    pub fn parse(raw: &str) -> Option<Self> {
        if !raw.starts_with('/') {
            return None;
        }

        let (before_fragment, fragment) = match raw.split_once('#') {
            Some((head, tail)) => (head, Some(tail.to_string())),
            None => (raw, None),
        };
        let (pathname, search) = match before_fragment.split_once('?') {
            Some((head, tail)) => (head, Some(tail.to_string())),
            None => (before_fragment, None),
        };

        let segments: Vec<&str> = pathname.split('/').filter(|s| !s.is_empty()).collect();
        let qualified_pathname = normalized_path(segments.iter().copied());
        let public_pathname = normalized_path(
            segments
                .iter()
                .copied()
                .filter(|segment| !is_group_segment(segment)),
        );

        Some(Self {
            qualified_pathname,
            location: Location {
                pathname: public_pathname,
                search,
                fragment,
            },
        })
    }
}

pub(crate) fn is_group_segment(segment: &str) -> bool {
    segment.len() >= 2 && segment.starts_with('(') && segment.ends_with(')')
}

fn normalized_path<'a>(segments: impl Iterator<Item = &'a str>) -> String {
    let joined = segments.collect::<Vec<_>>().join("/");
    if joined.is_empty() {
        "/".to_string()
    } else {
        format!("/{joined}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_qualifiers_are_not_public() {
        let destination = Destination::parse("/(tabs)/(search)/posts/42?q=rust#reply").unwrap();
        assert_eq!(destination.qualified_pathname, "/(tabs)/(search)/posts/42");
        assert_eq!(destination.location.pathname, "/posts/42");
        assert_eq!(destination.location.search.as_deref(), Some("q=rust"));
        assert_eq!(destination.location.fragment.as_deref(), Some("reply"));
        assert_eq!(destination.location.to_url(), "/posts/42?q=rust#reply");
    }
}
