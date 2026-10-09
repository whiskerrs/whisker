//! `whisker store push playstore` — send the `playstore` section of
//! `store.rs` to Google Play.
//!
//! Everything goes into one edit and becomes visible only when it is
//! committed; a failure part-way leaves Play untouched.

use anyhow::{Result, bail, ensure};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use whisker_config::store::{PlayDetails, PlayImages, PlayListing, PlayStore};

use crate::files;
use crate::play::Client;

const TITLE_LIMIT: usize = 30;
const SHORT_DESCRIPTION_LIMIT: usize = 80;
const FULL_DESCRIPTION_LIMIT: usize = 4000;
const SCREENSHOT_LIMIT: usize = 8;
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

/// Every image type `images` declares, as `(AppImageType, files)`.
fn image_types(images: &PlayImages) -> Vec<(&'static str, Vec<&str>)> {
    fn single(file: &Option<String>) -> Option<Vec<&str>> {
        file.as_deref().map(|file| vec![file])
    }
    fn list(files: &Option<Vec<String>>) -> Option<Vec<&str>> {
        files
            .as_ref()
            .map(|files| files.iter().map(String::as_str).collect())
    }
    [
        ("icon", single(&images.icon)),
        ("featureGraphic", single(&images.feature_graphic)),
        ("tvBanner", single(&images.tv_banner)),
        ("phoneScreenshots", list(&images.phone_screenshots)),
        ("sevenInchScreenshots", list(&images.seven_inch_screenshots)),
        ("tenInchScreenshots", list(&images.ten_inch_screenshots)),
        ("tvScreenshots", list(&images.tv_screenshots)),
        ("wearScreenshots", list(&images.wear_screenshots)),
    ]
    .into_iter()
    .filter_map(|(image_type, files)| Some((image_type, files?)))
    .collect()
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
    for images in &config.images {
        for (image_type, files) in image_types(images) {
            lines.push(format!(
                "images {} {image_type}: {} file(s)",
                images.language,
                files.len()
            ));
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

/// Refuse what Play would reject, before anything is sent. `root` is
/// the directory image paths are relative to.
pub fn validate(config: &PlayStore, root: &Path) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for images in &config.images {
        let language = &images.language;
        ensure!(
            seen.insert(language),
            "store.rs: playstore images `{language}` is declared twice"
        );
        for (image_type, files) in image_types(images) {
            ensure!(
                files.len() <= SCREENSHOT_LIMIT,
                "store.rs: playstore images `{language}` {image_type} has {} files; the limit \
                 is {SCREENSHOT_LIMIT}",
                files.len()
            );
            for file in files {
                files::image(
                    root,
                    file,
                    &format!("playstore images `{language}` {image_type}"),
                )?;
            }
        }
    }

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
    fn delete(&self, resource: &str) -> Result<()>;
    fn upload(&self, resource: &str, file: &Path, mime: &str) -> Result<()>;
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

    fn delete(&self, resource: &str) -> Result<()> {
        self.client.edit_delete(self.id, resource)
    }

    fn upload(&self, resource: &str, file: &Path, mime: &str) -> Result<()> {
        self.client
            .edit_upload(self.id, resource, file, mime)
            .map(|_| ())
    }
}

/// Make each declared image type hold exactly the declared files, in
/// order. A type whose hashes already match is left alone, so an
/// unchanged push uploads nothing.
fn apply_images(
    edit: &impl Edit,
    done: &mut Vec<String>,
    config: &PlayStore,
    root: &Path,
) -> Result<()> {
    if config.images.is_empty() {
        return Ok(());
    }
    let listings = edit.get("listings")?;
    let listed: Vec<&str> = listings
        .get("listings")
        .and_then(Value::as_array)
        .map(|listings| {
            listings
                .iter()
                .filter_map(|l| l.get("language")?.as_str())
                .collect()
        })
        .unwrap_or_default();
    for images in &config.images {
        let language = &images.language;
        // Play accepts images for a language without a listing and
        // silently drops them.
        ensure!(
            listed.contains(&language.as_str()),
            "store.rs: playstore images `{language}`: Google Play has no listing for this \
             language, so it would ignore the images — declare `p.listing(\"{language}\", …)` \
             with at least a title"
        );
        for (image_type, declared) in image_types(images) {
            let resource = format!("listings/{language}/{image_type}");
            let local: Vec<(PathBuf, &str)> = declared
                .iter()
                .map(|file| files::image(root, file, "playstore image"))
                .collect::<Result<_>>()?;
            let wanted: Vec<String> = local
                .iter()
                .map(|(path, _)| files::sha256_hex(path))
                .collect::<Result<_>>()?;
            let existing = edit.get(&resource)?;
            let existing: Vec<&str> = existing
                .get("images")
                .and_then(Value::as_array)
                .map(|images| {
                    images
                        .iter()
                        .filter_map(|image| image.get("sha256")?.as_str())
                        .collect()
                })
                .unwrap_or_default();
            if existing == wanted {
                done.push(format!("images {language} {image_type}: unchanged"));
                continue;
            }
            done.push(format!(
                "images {language} {image_type}: replaced {} with {}",
                existing.len(),
                local.len()
            ));
            if !existing.is_empty() {
                edit.delete(&resource)?;
            }
            for (path, mime) in &local {
                edit.upload(&resource, path, mime)?;
            }
        }
    }
    Ok(())
}

fn apply(edit: &impl Edit, config: &PlayStore, root: &Path) -> Result<Vec<String>> {
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
        apply_images(edit, &mut done, config, root)?;
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
    // After the listings: Play ignores images for a language that
    // has no listing yet.
    apply_images(edit, &mut done, config, root)?;
    Ok(done)
}

/// Send `config`'s details, listings and images in one committed
/// edit. Returns one line per resource written.
pub fn push(client: &Client, config: &PlayStore, root: &Path) -> Result<Vec<String>> {
    validate(config, root)?;
    if describe(config).is_empty() {
        bail!("store.rs declares no playstore details, listings or images — nothing to push");
    }
    let id = client.insert_edit()?;
    let result = apply(
        &LiveEdit {
            client,
            id: id.as_str(),
        },
        config,
        root,
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

    #[derive(Default)]
    struct FakeEdit {
        listings: Value,
        /// sha256 of the images Play already has, per resource.
        images: std::collections::BTreeMap<String, Vec<String>>,
        sent: RefCell<Vec<(String, String, Value)>>,
    }

    impl Edit for FakeEdit {
        fn get(&self, resource: &str) -> Result<Value> {
            Ok(match resource {
                "listings" => self.listings.clone(),
                "details" => json!({ "contactEmail": "old@example.com", "contactPhone": "" }),
                other => json!({ "images": self
                    .images
                    .get(other)
                    .into_iter()
                    .flatten()
                    .map(|sha256| json!({ "sha256": sha256 }))
                    .collect::<Vec<_>>() }),
            })
        }

        fn send(&self, method: &str, resource: &str, body: Value) -> Result<Value> {
            self.sent
                .borrow_mut()
                .push((method.to_string(), resource.to_string(), body));
            Ok(Value::Null)
        }

        fn delete(&self, resource: &str) -> Result<()> {
            self.sent
                .borrow_mut()
                .push(("DELETE".to_string(), resource.to_string(), Value::Null));
            Ok(())
        }

        fn upload(&self, resource: &str, file: &Path, mime: &str) -> Result<()> {
            let name = file.file_name().unwrap().to_string_lossy();
            self.sent.borrow_mut().push((
                "UPLOAD".to_string(),
                resource.to_string(),
                json!(format!("{name} {mime}")),
            ));
            Ok(())
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
            ..FakeEdit::default()
        };
        let done = apply(&edit, &config(), Path::new(".")).unwrap();
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
            ..FakeEdit::default()
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
        let err = validate(&config, Path::new(".")).unwrap_err().to_string();
        assert!(err.contains("`ja-JP` title is 31"), "{err}");

        let mut config = PlayStore::default();
        config.listing("ja-JP", |_| {}).listing("ja-JP", |_| {});
        assert!(
            validate(&config, Path::new("."))
                .unwrap_err()
                .to_string()
                .contains("declared twice")
        );
    }

    fn image_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in [("1.png", "one"), ("2.jpg", "two"), ("icon.png", "icon")] {
            std::fs::write(dir.path().join(name), bytes).unwrap();
        }
        dir
    }

    #[test]
    fn a_changed_image_type_is_replaced_whole_and_an_unchanged_one_is_skipped() {
        let dir = image_dir();
        let mut config = PlayStore::default();
        config.images("ja-JP", |i| {
            i.icon("icon.png").phone_screenshots(["1.png", "2.jpg"]);
        });
        let icon_hash = files::sha256_hex(&dir.path().join("icon.png")).unwrap();
        let edit = FakeEdit {
            listings: json!({ "listings": [{ "language": "ja-JP" }] }),
            images: [
                ("listings/ja-JP/icon".to_string(), vec![icon_hash]),
                (
                    "listings/ja-JP/phoneScreenshots".to_string(),
                    vec!["stale".to_string()],
                ),
            ]
            .into(),
            ..FakeEdit::default()
        };
        let done = apply(&edit, &config, dir.path()).unwrap();

        let resource = "listings/ja-JP/phoneScreenshots".to_string();
        assert_eq!(
            *edit.sent.borrow(),
            vec![
                ("DELETE".to_string(), resource.clone(), Value::Null),
                (
                    "UPLOAD".to_string(),
                    resource.clone(),
                    json!("1.png image/png")
                ),
                ("UPLOAD".to_string(), resource, json!("2.jpg image/jpeg")),
            ]
        );
        assert_eq!(
            done,
            vec![
                "images ja-JP icon: unchanged",
                "images ja-JP phoneScreenshots: replaced 1 with 2",
            ]
        );
    }

    #[test]
    fn images_for_a_language_play_has_no_listing_for_are_refused() {
        let dir = image_dir();
        let mut config = PlayStore::default();
        config.images("fr-FR", |i| {
            i.icon("icon.png");
        });
        let edit = FakeEdit {
            listings: json!({ "listings": [{ "language": "ja-JP" }] }),
            ..FakeEdit::default()
        };
        let err = apply(&edit, &config, dir.path()).unwrap_err().to_string();
        assert!(err.contains("has no listing for this language"), "{err}");
        assert!(edit.sent.borrow().is_empty());
    }

    #[test]
    fn image_files_are_checked_before_anything_is_sent() {
        let dir = image_dir();
        let mut config = PlayStore::default();
        config.images("ja-JP", |i| {
            i.phone_screenshots(["missing.png"]);
        });
        let err = validate(&config, dir.path()).unwrap_err().to_string();
        assert!(err.contains("`missing.png` does not exist"), "{err}");

        let mut config = PlayStore::default();
        config.images("ja-JP", |i| {
            i.icon("icon.gif");
        });
        let err = validate(&config, dir.path()).unwrap_err().to_string();
        assert!(err.contains("must be a .png, .jpg or .jpeg"), "{err}");

        let mut config = PlayStore::default();
        config.images("ja-JP", |i| {
            i.phone_screenshots(vec!["1.png"; 9]);
        });
        let err = validate(&config, dir.path()).unwrap_err().to_string();
        assert!(err.contains("has 9 files; the limit is 8"), "{err}");
    }
}
