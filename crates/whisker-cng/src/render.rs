//! `{{placeholder}}` substitution.
//!
//! Deliberately minimal — no conditionals, no escaping, no nested
//! constructs. Templates are project scaffolding; if a value needs
//! escaping (quote-heavy XML attributes, etc.) the renderer caller is
//! responsible. Errors are returned for unknown placeholders so a
//! typo'd `{{`-block fails loudly at sync time instead of writing a
//! broken file the user has to debug.

use anyhow::{bail, Result};
use std::collections::HashMap;

/// Replace every `{{key}}` in `template` with `vars[key]`. Returns
/// `Err` if a placeholder doesn't have a corresponding entry — that
/// almost always means a template was edited without updating the
/// renderer and we'd rather surface it now than ship a literal
/// `{{xyz}}` into the generated project.
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
}
