//! Formatter options — derived ENTIRELY from rustfmt's own settings.
//!
//! There are deliberately NO whisker-specific knobs here. Every field
//! mirrors a `rustfmt.toml` key, with rustfmt's documented default:
//!
//! | field        | rustfmt.toml key | default |
//! |--------------|------------------|---------|
//! | `max_width`  | `max_width`      | 100     |
//! | `tab_spaces` | `tab_spaces`     | 4       |
//! | `hard_tabs`  | `hard_tabs`      | false   |
//! | `edition`    | (CLI `--edition`)| 2015    |
//!
//! `format_source` lets the rustfmt *binary* read `rustfmt.toml` for
//! the base Rust pass (so any key rustfmt understands is honored). The
//! subset captured here is exactly the subset the macro-body
//! pretty-printer needs in order to match rustfmt's indentation /
//! wrapping. We resolve those few keys from the same `rustfmt.toml`
//! ([`FmtOptions::from_rustfmt_config`]) so the macro bodies line up
//! with the surrounding code.

/// Layout options for the macro-body pretty-printer. Mirrors the
/// rustfmt keys that affect indentation and wrapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FmtOptions {
    /// `max_width` — the column past which the printer tries to wrap.
    pub max_width: usize,
    /// `tab_spaces` — spaces per indent level (when `hard_tabs` is
    /// false). Also the width charged per `\t` when `hard_tabs` is on,
    /// for column accounting.
    pub tab_spaces: usize,
    /// `hard_tabs` — emit a literal tab per indent level instead of
    /// `tab_spaces` spaces.
    pub hard_tabs: bool,
    /// Rust edition to pass to the rustfmt binary (and to `syn` parse,
    /// though `syn` is edition-agnostic for our purposes). `None` lets
    /// rustfmt pick its own default.
    pub edition: Option<String>,
}

impl Default for FmtOptions {
    fn default() -> Self {
        // rustfmt's documented defaults.
        Self {
            max_width: 100,
            tab_spaces: 4,
            hard_tabs: false,
            edition: None,
        }
    }
}

impl FmtOptions {
    /// Render one indent *level* as a string fragment.
    pub(crate) fn indent_unit(&self) -> String {
        if self.hard_tabs {
            "\t".to_string()
        } else {
            " ".repeat(self.tab_spaces)
        }
    }

    /// The display width charged for `levels` indent levels — used for
    /// `max_width` accounting (a hard tab is charged `tab_spaces`).
    pub(crate) fn indent_width(&self, levels: usize) -> usize {
        levels * self.tab_spaces
    }

    /// Build an indent prefix of `levels` indent levels.
    pub(crate) fn indent_prefix(&self, levels: usize) -> String {
        self.indent_unit().repeat(levels)
    }

    /// Parse the handful of layout keys we care about out of a
    /// `rustfmt.toml` text blob. Unknown keys are ignored (rustfmt
    /// itself still sees them on the base pass). Missing keys keep
    /// their default.
    ///
    /// This is a tiny hand-rolled `key = value` reader rather than a
    /// full TOML parse so the crate doesn't take a `toml` dependency
    /// just for four scalar keys.
    pub fn from_rustfmt_config(toml_src: &str) -> Self {
        let mut opts = FmtOptions::default();
        for line in toml_src.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim_matches('\'');
            match key {
                "max_width" => {
                    if let Ok(v) = value.parse() {
                        opts.max_width = v;
                    }
                }
                "tab_spaces" => {
                    if let Ok(v) = value.parse() {
                        opts.tab_spaces = v;
                    }
                }
                "hard_tabs" => {
                    if let Ok(v) = value.parse() {
                        opts.hard_tabs = v;
                    }
                }
                "edition" => {
                    opts.edition = Some(value.to_string());
                }
                _ => {}
            }
        }
        opts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_rustfmt() {
        let o = FmtOptions::default();
        assert_eq!(o.max_width, 100);
        assert_eq!(o.tab_spaces, 4);
        assert!(!o.hard_tabs);
        assert_eq!(o.edition, None);
    }

    #[test]
    fn parses_rustfmt_toml_keys() {
        let toml = "max_width = 80\ntab_spaces = 2\nhard_tabs = true\nedition = \"2021\"\n";
        let o = FmtOptions::from_rustfmt_config(toml);
        assert_eq!(o.max_width, 80);
        assert_eq!(o.tab_spaces, 2);
        assert!(o.hard_tabs);
        assert_eq!(o.edition.as_deref(), Some("2021"));
    }

    #[test]
    fn ignores_comments_and_unknown_keys() {
        let toml = "# a comment\nimports_granularity = \"Crate\"\ntab_spaces = 2\n";
        let o = FmtOptions::from_rustfmt_config(toml);
        assert_eq!(o.tab_spaces, 2);
        assert_eq!(o.max_width, 100); // default retained
    }

    #[test]
    fn indent_unit_spaces_vs_tabs() {
        let mut o = FmtOptions {
            tab_spaces: 2,
            ..FmtOptions::default()
        };
        assert_eq!(o.indent_unit(), "  ");
        assert_eq!(o.indent_prefix(3), "      ");
        o.hard_tabs = true;
        assert_eq!(o.indent_unit(), "\t");
        assert_eq!(o.indent_prefix(2), "\t\t");
        // width accounting still charges tab_spaces per level
        assert_eq!(o.indent_width(2), 4);
    }
}
