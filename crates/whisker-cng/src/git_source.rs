//! Git sources of resolved packages, restated as Cargo dependency fields.
//!
//! Cargo keys a git source by its URL and reference kind, so a generated Host
//! that names the same repository through `rev` while the application uses
//! `branch` gets a second copy of every crate in it. Dependencies on a git
//! package therefore repeat the application's reference exactly.

/// Repository and reference of a package that Cargo resolved from git.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct GitSource {
    pub url: String,
    /// `branch`, `tag`, or `rev` with its value; `None` follows the default branch.
    pub reference: Option<(String, String)>,
}

impl GitSource {
    /// Parse Cargo's `git+<url>[?<kind>=<value>]#<commit>` source id.
    pub fn parse(repr: &str) -> Option<Self> {
        let rest = repr.strip_prefix("git+")?;
        let rest = rest.split_once('#').map_or(rest, |(location, _)| location);
        let (url, query) = rest
            .split_once('?')
            .map_or((rest, None), |(u, q)| (u, Some(q)));
        let reference = query.and_then(|query| {
            query.split('&').find_map(|pair| {
                let (kind, value) = pair.split_once('=')?;
                matches!(kind, "branch" | "tag" | "rev")
                    .then(|| (kind.to_owned(), percent_decode(value)))
            })
        });
        Some(Self {
            url: url.to_owned(),
            reference,
        })
    }

    /// The fields that select this source inside a Cargo dependency table.
    pub fn dependency_fields(&self) -> String {
        match &self.reference {
            Some((kind, value)) => format!("git = {:?}, {kind} = {value:?}", self.url),
            None => format!("git = {:?}", self.url),
        }
    }
}

/// The git source of the `whisker` crate the application resolved, if any.
pub(crate) fn whisker(metadata: &cargo_metadata::Metadata) -> Option<GitSource> {
    let selected = metadata.resolve.as_ref()?;
    metadata
        .packages
        .iter()
        .filter(|package| package.name == "whisker")
        .filter(|package| selected.nodes.iter().any(|node| node.id == package.id))
        .find_map(|package| GitSource::parse(&package.source.as_ref()?.repr))
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escaped = (bytes[index] == b'%')
            .then(|| bytes.get(index + 1..index + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok());
        match escaped {
            Some(byte) => {
                decoded.push(byte);
                index += 3;
            }
            None => {
                decoded.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_sources_keep_their_branch() {
        let source = GitSource::parse(
            "git+https://github.com/whiskerrs/whisker?branch=feature%2Fhost#1dc9ebcdbaba82ec",
        )
        .unwrap();
        assert_eq!(
            source.dependency_fields(),
            r#"git = "https://github.com/whiskerrs/whisker", branch = "feature/host""#
        );
    }

    #[test]
    fn rev_and_tag_sources_keep_their_kind() {
        let rev = GitSource::parse("git+https://example.com/a.git?rev=abc123#abc123def").unwrap();
        assert_eq!(rev.reference, Some(("rev".into(), "abc123".into())));
        let tag = GitSource::parse("git+https://example.com/a.git?tag=v1.0#0123").unwrap();
        assert_eq!(tag.reference, Some(("tag".into(), "v1.0".into())));
    }

    #[test]
    fn default_branch_sources_name_only_the_repository() {
        let source = GitSource::parse("git+https://example.com/a.git#0123abcd").unwrap();
        assert_eq!(
            source.dependency_fields(),
            r#"git = "https://example.com/a.git""#
        );
    }

    #[test]
    fn registry_and_path_sources_are_not_git() {
        assert!(
            GitSource::parse("registry+https://github.com/rust-lang/crates.io-index").is_none()
        );
        assert!(GitSource::parse("path+file:///tmp/whisker").is_none());
    }
}
