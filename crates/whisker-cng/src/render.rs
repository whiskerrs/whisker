//! `{{placeholder}}` substitution.
//!
//! Deliberately minimal — no conditionals, no escaping, no nested
//! constructs. Callers escape their own values (see [`escape_xml`]).
//! An unknown placeholder is an error so a typo'd `{{`-block fails at
//! sync time instead of shipping a broken generated file.

use anyhow::{Result, bail};
use std::collections::HashMap;

/// Escape the five XML special characters (`&`, `<`, `>`, `"`, `'`),
/// for plugin-supplied strings the `template_vars` builders emit into
/// hand-rolled XML (Info.plist, AndroidManifest.xml).
pub(crate) fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// Reject plugin-supplied `extra_files` paths that would escape the
/// gen root: absolute paths, `..` traversal, and Windows drive
/// prefixes (caught by `is_absolute` on Windows hosts). Called before
/// writing any `extra_files` entry, so a buggy plugin can't drop a
/// file outside the workspace.
pub(crate) fn validate_extra_file_path(path: &std::path::Path) -> anyhow::Result<()> {
    if path.is_absolute() {
        anyhow::bail!(
            "extra_files path must be relative to the gen root: `{}`",
            path.display(),
        );
    }
    for component in path.components() {
        if matches!(component, std::path::Component::ParentDir) {
            anyhow::bail!(
                "extra_files path must not contain `..` (would escape gen root): `{}`",
                path.display(),
            );
        }
    }
    Ok(())
}

/// Replace every `{{key}}` in `template` with `vars[key]`. A
/// placeholder with no entry is an `Err` — it almost always means a
/// template was edited without updating the renderer.
pub fn render(template: &str, vars: &HashMap<&'static str, String>) -> Result<String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];
        let Some(end) = after_open.find("}}") else {
            bail!("unterminated {{{{ at byte offset {start}");
        };
        let key = after_open[..end].trim();
        let Some(val) = vars.get(key) else {
            bail!("unknown template placeholder: `{{{{ {key} }}}}`");
        };
        out.push_str(val);
        rest = &after_open[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&'static str, &str)]) -> HashMap<&'static str, String> {
        pairs.iter().map(|(k, v)| (*k, v.to_string())).collect()
    }

    #[test]
    fn renders_a_single_placeholder() {
        let out = render("Hello, {{name}}!", &vars(&[("name", "world")])).unwrap();
        assert_eq!(out, "Hello, world!");
    }

    #[test]
    fn renders_multiple_placeholders() {
        let out = render(
            "{{greeting}}, {{name}}!",
            &vars(&[("greeting", "Hi"), ("name", "Whisker")]),
        )
        .unwrap();
        assert_eq!(out, "Hi, Whisker!");
    }

    #[test]
    fn passes_through_unbraced_text_unchanged() {
        let out = render("no placeholders here", &HashMap::new()).unwrap();
        assert_eq!(out, "no placeholders here");
    }

    #[test]
    fn allows_whitespace_inside_braces() {
        let out = render("{{ name }}", &vars(&[("name", "ok")])).unwrap();
        assert_eq!(out, "ok");
    }

    #[test]
    fn errors_on_unknown_placeholder() {
        let err = render("{{missing}}", &HashMap::new()).unwrap_err();
        assert!(err.to_string().contains("missing"), "got: {err:#}");
    }

    #[test]
    fn errors_on_unterminated_open_brace() {
        let err = render("hello {{name", &vars(&[("name", "x")])).unwrap_err();
        assert!(err.to_string().contains("unterminated"), "got: {err:#}");
    }

    use std::path::PathBuf;

    #[test]
    fn validate_extra_file_path_accepts_simple_relative_paths() {
        validate_extra_file_path(&PathBuf::from("Sources/Helper.swift")).unwrap();
        validate_extra_file_path(&PathBuf::from("app/google-services.json")).unwrap();
        validate_extra_file_path(&PathBuf::from("file.txt")).unwrap();
    }

    #[test]
    fn validate_extra_file_path_rejects_absolute_paths() {
        let err = validate_extra_file_path(&PathBuf::from("/etc/passwd")).unwrap_err();
        assert!(err.to_string().contains("must be relative"), "{err}");
    }

    #[test]
    fn validate_extra_file_path_rejects_parent_dir_traversal() {
        let err = validate_extra_file_path(&PathBuf::from("../../escape")).unwrap_err();
        assert!(err.to_string().contains(".."), "{err}");
    }

    #[test]
    fn validate_extra_file_path_rejects_middle_dot_dot() {
        let err = validate_extra_file_path(&PathBuf::from("Sources/../escape")).unwrap_err();
        assert!(err.to_string().contains(".."), "{err}");
    }
}
