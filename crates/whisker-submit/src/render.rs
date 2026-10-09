//! Render a store config back into `store.rs` source, for
//! `whisker store pull`. The output is a complete file that
//! reproduces the config it was rendered from.

use whisker_config::store::{AppStore, PlayStore, ReviewDetail};

/// A Rust string literal for `text`: a plain one when it fits on a
/// line, a raw one otherwise so long store text stays readable.
fn literal(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    if !text.contains(['\n', '"', '\\']) {
        return format!("\"{text}\"");
    }
    // A raw string ends at `"` followed by its own number of `#`, so
    // use one more than the text ever has after a quote.
    let mut hashes = 1;
    for (index, _) in text.match_indices('"') {
        let run = text[index + 1..].chars().take_while(|c| *c == '#').count();
        hashes = hashes.max(run + 1);
    }
    let hashes = "#".repeat(hashes);
    format!("r{hashes}\"{text}\"{hashes}")
}

/// Source lines with indentation tracked by block depth.
struct Source {
    text: String,
    depth: usize,
}

impl Source {
    fn line(&mut self, line: &str) {
        self.text.push_str(&"    ".repeat(self.depth));
        self.text.push_str(line);
        self.text.push('\n');
    }

    /// `header {` … `});` around `body`, skipped entirely when the
    /// body writes nothing.
    fn block(&mut self, header: &str, body: impl FnOnce(&mut Self)) {
        let before = self.text.len();
        self.line(&format!("{header} {{"));
        let opened = self.text.len();
        self.depth += 1;
        body(self);
        self.depth -= 1;
        if self.text.len() == opened {
            self.text.truncate(before);
        } else {
            self.line("});");
        }
    }

    /// `receiver.name(value);` for each set attribute.
    fn setters(&mut self, receiver: &str, attributes: &[(&str, &Option<String>)]) {
        for (name, value) in attributes {
            if let Some(value) = value {
                self.line(&format!("{receiver}.{name}({});", literal(value)));
            }
        }
    }

    fn review_detail(&mut self, detail: &ReviewDetail) {
        self.setters(
            "r",
            &[
                ("contact_first_name", &detail.contact_first_name),
                ("contact_last_name", &detail.contact_last_name),
                ("contact_phone", &detail.contact_phone),
                ("contact_email", &detail.contact_email),
                ("notes", &detail.notes),
            ],
        );
        if let Some(required) = detail.demo_account_required {
            self.line(&format!("r.demo_account_required({required});"));
        }
    }
}

fn file(section: impl FnOnce(&mut Source)) -> String {
    let mut source = Source {
        text: String::new(),
        depth: 0,
    };
    source.line("fn main() {");
    source.depth = 1;
    source.block("whisker::store::run(|store|", section);
    source.depth = 0;
    source.line("}");
    source.text
}

pub fn playstore(config: &PlayStore) -> String {
    file(|s| {
        s.block("store.playstore(|p|", |s| {
            s.block("p.details(|d|", |s| {
                s.setters(
                    "d",
                    &[
                        ("default_language", &config.details.default_language),
                        ("contact_email", &config.details.contact_email),
                        ("contact_phone", &config.details.contact_phone),
                        ("contact_website", &config.details.contact_website),
                    ],
                );
            });
            for listing in &config.listings {
                s.block(
                    &format!("p.listing({}, |l|", literal(&listing.language)),
                    |s| {
                        s.setters(
                            "l",
                            &[
                                ("title", &listing.title),
                                ("short_description", &listing.short_description),
                                ("full_description", &listing.full_description),
                                ("video", &listing.video),
                            ],
                        );
                    },
                );
            }
        });
    })
}

pub fn appstore(config: &AppStore) -> String {
    file(|s| {
        s.block("store.appstore(|a|", |s| {
            s.block("a.app(|app|", |s| {
                s.setters(
                    "app",
                    &[
                        (
                            "content_rights_declaration",
                            &config.app.content_rights_declaration,
                        ),
                        ("primary_locale", &config.app.primary_locale),
                    ],
                );
            });
            let info = &config.app_info;
            s.block("a.app_info(|i|", |s| {
                s.setters(
                    "i",
                    &[
                        ("primary_category", &info.primary_category),
                        ("primary_subcategory_one", &info.primary_subcategory_one),
                        ("primary_subcategory_two", &info.primary_subcategory_two),
                        ("secondary_category", &info.secondary_category),
                        ("secondary_subcategory_one", &info.secondary_subcategory_one),
                        ("secondary_subcategory_two", &info.secondary_subcategory_two),
                    ],
                );
                for l in &info.localizations {
                    s.block(&format!("i.locale({}, |l|", literal(&l.locale)), |s| {
                        s.setters(
                            "l",
                            &[
                                ("name", &l.name),
                                ("subtitle", &l.subtitle),
                                ("privacy_policy_url", &l.privacy_policy_url),
                                ("privacy_choices_url", &l.privacy_choices_url),
                                ("privacy_policy_text", &l.privacy_policy_text),
                            ],
                        );
                    });
                }
            });
            let version = &config.version;
            s.block("a.version(|v|", |s| {
                s.setters(
                    "v",
                    &[
                        ("copyright", &version.copyright),
                        ("release_type", &version.release_type),
                        ("earliest_release_date", &version.earliest_release_date),
                    ],
                );
                for l in &version.localizations {
                    s.block(&format!("v.locale({}, |l|", literal(&l.locale)), |s| {
                        s.setters(
                            "l",
                            &[
                                ("description", &l.description),
                                ("keywords", &l.keywords),
                                ("whats_new", &l.whats_new),
                                ("promotional_text", &l.promotional_text),
                                ("marketing_url", &l.marketing_url),
                                ("support_url", &l.support_url),
                            ],
                        );
                    });
                }
            });
            s.block("a.review_detail(|r|", |s| {
                s.review_detail(&config.review_detail)
            });
            s.block("a.beta_app(|b|", |s| {
                for l in &config.beta_app.localizations {
                    s.block(&format!("b.locale({}, |l|", literal(&l.locale)), |s| {
                        s.setters(
                            "l",
                            &[
                                ("description", &l.description),
                                ("feedback_email", &l.feedback_email),
                                ("marketing_url", &l.marketing_url),
                                ("privacy_policy_url", &l.privacy_policy_url),
                                ("tv_os_privacy_policy", &l.tv_os_privacy_policy),
                            ],
                        );
                    });
                }
            });
            s.block("a.beta_app_review_detail(|r|", |s| {
                s.review_detail(&config.beta_app_review_detail)
            });
        });
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_stay_valid_for_any_store_text() {
        assert_eq!(literal("GIGA"), "\"GIGA\"");
        assert_eq!(literal("a\r\nb"), "r#\"a\nb\"#");
        assert_eq!(literal("say \"hi\""), "r#\"say \"hi\"\"#");
        // The text itself contains `"#`, which would end `r#"…"#` early.
        assert_eq!(literal("x\"#y"), "r##\"x\"#y\"##");
        assert_eq!(literal("back\\slash"), "r#\"back\\slash\"#");
    }

    #[test]
    fn playstore_renders_only_what_is_set() {
        let mut config = PlayStore::default();
        config
            .details(|d| {
                d.contact_email("support@example.com");
            })
            .listing("ja-JP", |l| {
                l.title("アプリ").full_description("一行目\n二行目");
            })
            .listing("de-DE", |_| {});
        assert_eq!(
            playstore(&config),
            r##"fn main() {
    whisker::store::run(|store| {
        store.playstore(|p| {
            p.details(|d| {
                d.contact_email("support@example.com");
            });
            p.listing("ja-JP", |l| {
                l.title("アプリ");
                l.full_description(r#"一行目
二行目"#);
            });
        });
    });
}
"##
        );
    }

    #[test]
    fn appstore_renders_nested_locales_and_booleans() {
        let mut config = AppStore::default();
        config
            .app_info(|i| {
                i.primary_category("BOOKS");
                i.locale("ja", |l| {
                    l.name("アプリ");
                });
            })
            .beta_app_review_detail(|r| {
                r.demo_account_required(false);
            });
        assert_eq!(
            appstore(&config),
            r#"fn main() {
    whisker::store::run(|store| {
        store.appstore(|a| {
            a.app_info(|i| {
                i.primary_category("BOOKS");
                i.locale("ja", |l| {
                    l.name("アプリ");
                });
            });
            a.beta_app_review_detail(|r| {
                r.demo_account_required(false);
            });
        });
    });
}
"#
        );
    }

    #[test]
    fn an_empty_store_renders_an_empty_program() {
        assert_eq!(playstore(&PlayStore::default()), "fn main() {\n}\n");
    }
}
