//! `whisker store push playstore` — send the `playstore` section of
//! `store.rs` to Google Play.
//!
//! Everything goes into one edit and becomes visible only when it is
//! committed; a failure part-way leaves Play untouched.

use anyhow::{Result, bail, ensure};
use serde_json::{Map, Value, json};
use whisker_config::store::{PlayDetails, PlayListing, PlayStore};

use crate::play::Client;

const TITLE_LIMIT: usize = 30;
const SHORT_DESCRIPTION_LIMIT: usize = 80;
const FULL_DESCRIPTION_LIMIT: usize = 4000;
/// For `whisker submit android`, which sends the release notes.
pub const RELEASE_NOTES_LIMIT: usize = 500;

fn set_fields(pairs: &[(&str, &Option<String>)]) -> Map<String, Value> {
    pairs
        .iter()
        .filter_map(|(name, value)| Some((name.to_string(), json!(value.as_deref()?))))
        .collect()
}

fn details_fields(details: &PlayDetails) -> Map<String, Value> {
    set_fields(&[
        ("defaultLanguage", &details.default_language),
        ("contactEmail", &details.contact_email),
        ("contactPhone", &details.contact_phone),
        ("contactWebsite", &details.contact_website),
    ])
}

fn listing_fields(listing: &PlayListing) -> Map<String, Value> {
    set_fields(&[
        ("title", &listing.title),
        ("shortDescription", &listing.short_description),
        ("fullDescription", &listing.full_description),
        ("video", &listing.video),
    ])
}

fn names(fields: &Map<String, Value>) -> String {
    fields.keys().cloned().collect::<Vec<_>>().join(", ")
}

/// One line per resource `push` would write.
pub fn describe(config: &PlayStore) -> Vec<String> {
    let mut lines = Vec::new();
    let details = details_fields(&config.details);
    if !details.is_empty() {
        lines.push(format!("details: {}", names(&details)));
    }
    for listing in &config.listings {
        let fields = listing_fields(listing);
        if !fields.is_empty() {
            lines.push(format!("listings {}: {}", listing.language, names(&fields)));
        }
    }
    lines
}

fn check_length(what: &str, value: &Option<String>, limit: usize) -> Result<()> {
    if let Some(value) = value {
        let length = value.trim().chars().count();
        ensure!(
            length <= limit,
            "store.rs: playstore {what} is {length} characters; the limit is {limit}"
        );
    }
    Ok(())
}

/// Refuse what Play would reject, before anything is sent.
pub fn validate(config: &PlayStore) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for listing in &config.listings {
        let language = &listing.language;
        ensure!(
            seen.insert(language),
            "store.rs: playstore listing `{language}` is declared twice"
        );
        check_length(
            &format!("listing `{language}` title"),
            &listing.title,
            TITLE_LIMIT,
        )?;
        check_length(
            &format!("listing `{language}` short_description"),
            &listing.short_description,
            SHORT_DESCRIPTION_LIMIT,
        )?;
        check_length(
            &format!("listing `{language}` full_description"),
            &listing.full_description,
            FULL_DESCRIPTION_LIMIT,
        )?;
    }
    Ok(())
}

/// The calls one edit is made of; lets the sync logic run against a
/// recorded fake.
trait Edit {
    fn get(&self, resource: &str) -> Result<Value>;
    fn send(&self, method: &str, resource: &str, body: Value) -> Result<Value>;
}

struct LiveEdit<'a> {
    client: &'a Client<'a>,
    id: &'a str,
}

impl Edit for LiveEdit<'_> {
    fn get(&self, resource: &str) -> Result<Value> {
        self.client.edit_get(self.id, resource)
    }

    fn send(&self, method: &str, resource: &str, body: Value) -> Result<Value> {
        self.client.edit_send(self.id, method, resource, body)
    }
}

fn apply(edit: &impl Edit, config: &PlayStore) -> Result<Vec<String>> {
    let mut done = Vec::new();
    let details = details_fields(&config.details);
    if !details.is_empty() {
        done.push(format!("details: {}", names(&details)));
        edit.send("PATCH", "details", Value::Object(details))?;
    }

    let declared: Vec<_> = config
        .listings
        .iter()
        .map(|listing| (listing, listing_fields(listing)))
        .filter(|(_, fields)| !fields.is_empty())
        .collect();
    if declared.is_empty() {
        return Ok(done);
    }
    let existing = edit.get("listings")?;
    let existing: Vec<&str> = existing
        .get("listings")
        .and_then(Value::as_array)
        .map(|listings| {
            listings
                .iter()
                .filter_map(|l| l.get("language")?.as_str())
                .collect()
        })
        .unwrap_or_default();
    for (listing, mut fields) in declared {
        let language = listing.language.as_str();
        // PATCH only updates; a language Play doesn't have yet has to
        // be created with PUT.
        let (method, verb) = if existing.contains(&language) {
            ("PATCH", "updated")
        } else {
            ("PUT", "created")
        };
        done.push(format!("listings {language} ({verb}): {}", names(&fields)));
        fields.insert("language".into(), json!(language));
        edit.send(
            method,
            &format!("listings/{language}"),
            Value::Object(fields),
        )?;
    }
    Ok(done)
}

/// Send `config`'s details and listings in one committed edit.
/// Returns one line per resource written.
pub fn push(client: &Client, config: &PlayStore) -> Result<Vec<String>> {
    validate(config)?;
    if describe(config).is_empty() {
        bail!("store.rs declares no playstore details or listings — nothing to push");
    }
    let id = client.insert_edit()?;
    let result = apply(
        &LiveEdit {
            client,
            id: id.as_str(),
        },
        config,
    )
    .and_then(|done| client.commit_edit(&id).map(|()| done));
    if result.is_err() {
        // Best-effort: an abandoned edit expires on its own.
        client.delete_edit(&id).ok();
    }
    result
}

fn text(value: &Value, name: &str) -> Option<String> {
    value
        .get(name)?
        .as_str()
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn read(edit: &impl Edit) -> Result<PlayStore> {
    let details = edit.get("details")?;
    let listings = edit.get("listings")?;
    Ok(PlayStore {
        details: PlayDetails {
            default_language: text(&details, "defaultLanguage"),
            contact_email: text(&details, "contactEmail"),
            contact_phone: text(&details, "contactPhone"),
            contact_website: text(&details, "contactWebsite"),
        },
        listings: listings
            .get("listings")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|listing| PlayListing {
                language: text(listing, "language").unwrap_or_default(),
                title: text(listing, "title"),
                short_description: text(listing, "shortDescription"),
                full_description: text(listing, "fullDescription"),
                video: text(listing, "video"),
            })
            .collect(),
        ..PlayStore::default()
    })
}

/// Read what Play holds for everything `push` writes. An edit is the
/// only way to read listings; it is discarded, never committed.
pub fn pull(client: &Client) -> Result<PlayStore> {
    let id = client.insert_edit()?;
    let result = read(&LiveEdit {
        client,
        id: id.as_str(),
    });
    client.delete_edit(&id).ok();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct FakeEdit {
        listings: Value,
        sent: RefCell<Vec<(String, String, Value)>>,
    }

    impl Edit for FakeEdit {
        fn get(&self, resource: &str) -> Result<Value> {
            Ok(match resource {
                "listings" => self.listings.clone(),
                _ => json!({ "contactEmail": "old@example.com", "contactPhone": "" }),
            })
        }

        fn send(&self, method: &str, resource: &str, body: Value) -> Result<Value> {
            self.sent
                .borrow_mut()
                .push((method.to_string(), resource.to_string(), body));
            Ok(Value::Null)
        }
    }

    fn config() -> PlayStore {
        let mut config = PlayStore::default();
        config
            .details(|d| {
                d.contact_email("support@example.com");
            })
            .listing("ja-JP", |l| {
                l.title("アプリ");
            })
            .listing("fr-FR", |l| {
                l.title("Appli").short_description("Courte");
            })
            .listing("de-DE", |_| {});
        config
    }

    #[test]
    fn only_set_fields_are_sent_and_new_languages_are_created() {
        let edit = FakeEdit {
            listings: json!({ "listings": [{ "language": "ja-JP", "title": "旧" }] }),
            sent: RefCell::new(Vec::new()),
        };
        let done = apply(&edit, &config()).unwrap();
        assert_eq!(
            *edit.sent.borrow(),
            vec![
                (
                    "PATCH".to_string(),
                    "details".to_string(),
                    json!({ "contactEmail": "support@example.com" })
                ),
                (
                    "PATCH".to_string(),
                    "listings/ja-JP".to_string(),
                    json!({ "language": "ja-JP", "title": "アプリ" })
                ),
                (
                    "PUT".to_string(),
                    "listings/fr-FR".to_string(),
                    json!({ "language": "fr-FR", "title": "Appli", "shortDescription": "Courte" })
                ),
            ]
        );
        assert_eq!(done.len(), 3);
        assert!(done[2].contains("fr-FR (created)"));
    }

    #[test]
    fn pull_reads_back_the_shape_push_writes_and_drops_empty_values() {
        let edit = FakeEdit {
            listings: json!({ "listings": [
                { "language": "ja-JP", "title": "旧", "fullDescription": "説明", "video": "" },
            ]}),
            sent: RefCell::new(Vec::new()),
        };
        let pulled = read(&edit).unwrap();
        let mut expected = PlayStore::default();
        expected
            .details(|d| {
                d.contact_email("old@example.com");
            })
            .listing("ja-JP", |l| {
                l.title("旧").full_description("説明");
            });
        assert_eq!(pulled, expected);
        assert!(edit.sent.borrow().is_empty(), "pull must not write");
    }

    #[test]
    fn describe_lists_only_what_would_be_written() {
        assert_eq!(
            describe(&config()),
            vec![
                "details: contactEmail",
                "listings ja-JP: title",
                "listings fr-FR: shortDescription, title",
            ]
        );
        assert!(describe(&PlayStore::default()).is_empty());
    }

    #[test]
    fn limits_and_duplicates_are_refused() {
        let mut config = PlayStore::default();
        config.listing("ja-JP", |l| {
            l.title("あ".repeat(31));
        });
        let err = validate(&config).unwrap_err().to_string();
        assert!(err.contains("`ja-JP` title is 31"), "{err}");

        let mut config = PlayStore::default();
        config.listing("ja-JP", |_| {}).listing("ja-JP", |_| {});
        assert!(
            validate(&config)
                .unwrap_err()
                .to_string()
                .contains("declared twice")
        );
    }
}
