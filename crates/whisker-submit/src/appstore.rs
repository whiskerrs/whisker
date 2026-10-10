//! `whisker store push appstore` — send the `appstore` section of
//! `store.rs` to App Store Connect.
//!
//! App Store Connect has no transaction: each resource is written on
//! its own. Everything that can be checked is therefore checked
//! first — limits, then that the app, an editable app info and an
//! editable version all exist — so a push fails before its first
//! write rather than half-way through.

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde_json::{Map, Value, json};
use std::path::Path;
use whisker_config::store::{
    AppInfoLocalization, AppStore, AppStoreVersionLocalization, AssetLibraryImage,
    BetaAppLocalization, Placements, ReviewDetail,
};

use crate::asc::{self, KeyAuth, UploadOperation};
use crate::files;

const NAME_LIMIT: usize = 30;
const SUBTITLE_LIMIT: usize = 30;
const DESCRIPTION_LIMIT: usize = 4000;
// Counted in characters although Apple's reference says bytes: App
// Store Connect holds live 82-character Thai keywords that are 184
// bytes of UTF-8.
const KEYWORDS_LIMIT: usize = 100;
const PROMOTIONAL_TEXT_LIMIT: usize = 170;
const WHATS_NEW_LIMIT: usize = 4000;
const REVIEW_NOTES_LIMIT: usize = 4000;
/// For `whisker submit ios`, which sends the build's "What to Test".
pub const BETA_WHATS_NEW_LIMIT: usize = 4000;

/// States in which App Store Connect accepts metadata changes.
const EDITABLE_APP_INFO_STATES: [&str; 4] = [
    "PREPARE_FOR_SUBMISSION",
    "DEVELOPER_REJECTED",
    "REJECTED",
    "READY_FOR_REVIEW",
];
const EDITABLE_VERSION_STATES: [&str; 6] = [
    "PREPARE_FOR_SUBMISSION",
    "DEVELOPER_REJECTED",
    "REJECTED",
    "METADATA_REJECTED",
    "INVALID_BINARY",
    "READY_FOR_REVIEW",
];

/// The locales App Store Connect accepts. Checked up front because a
/// bad code would otherwise fail on its own request, after earlier
/// locales were already written — there is no transaction to roll
/// back.
const LOCALES: [&str; 50] = [
    "ar-SA", "bn-BD", "ca", "cs", "da", "de-DE", "el", "en-AU", "en-CA", "en-GB", "en-US", "es-ES",
    "es-MX", "fi", "fr-CA", "fr-FR", "gu-IN", "he", "hi", "hr", "hu", "id", "it", "ja", "kn-IN",
    "ko", "ml-IN", "mr-IN", "ms", "nl-NL", "no", "or-IN", "pa-IN", "pl", "pt-BR", "pt-PT", "ro",
    "ru", "sk", "sl-SI", "sv", "ta-IN", "te-IN", "th", "tr", "uk", "ur-PK", "vi", "zh-Hans",
    "zh-Hant",
];

pub struct PushOptions<'a> {
    /// Create this version when the app has none being prepared.
    pub create_version: Option<&'a str>,
}

/// The calls a push is made of; lets the sync logic run against a
/// recorded fake.
pub trait Api {
    /// `None` when the resource does not exist.
    fn get(&self, path: &str) -> Result<Option<Value>>;
    /// `Value::Null` sends no body.
    fn send(&self, method: &str, path: &str, body: Value) -> Result<Value>;
    /// Transfer `file` to the URLs a reservation handed back.
    fn upload(&self, file: &Path, size: u64, operations: &[UploadOperation]) -> Result<()>;
    /// Wait between two polls of a processing asset.
    fn pause(&self) {
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

/// How many times a processing image is polled before giving up
/// (about five minutes).
const PROCESSING_POLLS: usize = 150;

impl Api for KeyAuth<'_> {
    fn get(&self, path: &str) -> Result<Option<Value>> {
        asc::get_optional(self, path)
    }

    fn send(&self, method: &str, path: &str, body: Value) -> Result<Value> {
        asc::request(self, method, path, (!body.is_null()).then_some(body))
    }

    fn upload(&self, file: &Path, size: u64, operations: &[UploadOperation]) -> Result<()> {
        asc::upload_parts(file, size, operations)
    }
}

const DEFAULT_IMAGE_CATEGORY: &str = "APP_SCREENSHOTS_AND_PREVIEWS";

fn category(image: &AssetLibraryImage) -> &str {
    image.category.as_deref().unwrap_or(DEFAULT_IMAGE_CATEGORY)
}

/// Every placement group `config` declares, with its locale.
fn placements(config: &AppStore) -> impl Iterator<Item = (&str, &Placements)> {
    config
        .version
        .localizations
        .iter()
        .flat_map(|l| l.placements.iter().map(|p| (l.locale.as_str(), p)))
}

fn set_fields(pairs: &[(&str, &Option<String>)]) -> Map<String, Value> {
    pairs
        .iter()
        .filter_map(|(name, value)| Some((name.to_string(), json!(value.as_deref()?))))
        .collect()
}

fn review_fields(detail: &ReviewDetail) -> Map<String, Value> {
    let mut fields = set_fields(&[
        ("contactFirstName", &detail.contact_first_name),
        ("contactLastName", &detail.contact_last_name),
        ("contactPhone", &detail.contact_phone),
        ("contactEmail", &detail.contact_email),
        ("notes", &detail.notes),
    ]);
    if let Some(required) = detail.demo_account_required {
        fields.insert("demoAccountRequired".into(), json!(required));
    }
    fields
}

/// `store.rs`'s `appstore` section as the attribute maps each
/// resource receives; empty maps and locales with nothing set are
/// what "not declared" looks like.
struct Plan {
    app: Map<String, Value>,
    /// `(relationship, appCategories id)`.
    categories: Vec<(&'static str, String)>,
    app_info_localizations: Vec<(String, Map<String, Value>)>,
    version: Map<String, Value>,
    version_localizations: Vec<(String, Map<String, Value>)>,
    review_detail: Map<String, Value>,
    beta_app_localizations: Vec<(String, Map<String, Value>)>,
    beta_app_review_detail: Map<String, Value>,
}

impl Plan {
    fn new(config: &AppStore) -> Self {
        fn declared(
            localizations: impl Iterator<Item = (String, Map<String, Value>)>,
        ) -> Vec<(String, Map<String, Value>)> {
            localizations
                .filter(|(_, fields)| !fields.is_empty())
                .collect()
        }
        let info = &config.app_info;
        Self {
            app: set_fields(&[
                (
                    "contentRightsDeclaration",
                    &config.app.content_rights_declaration,
                ),
                ("primaryLocale", &config.app.primary_locale),
            ]),
            categories: [
                ("primaryCategory", &info.primary_category),
                ("primarySubcategoryOne", &info.primary_subcategory_one),
                ("primarySubcategoryTwo", &info.primary_subcategory_two),
                ("secondaryCategory", &info.secondary_category),
                ("secondarySubcategoryOne", &info.secondary_subcategory_one),
                ("secondarySubcategoryTwo", &info.secondary_subcategory_two),
            ]
            .into_iter()
            .filter_map(|(relationship, id)| Some((relationship, id.clone()?)))
            .collect(),
            app_info_localizations: declared(info.localizations.iter().map(|l| {
                (
                    l.locale.clone(),
                    set_fields(&[
                        ("name", &l.name),
                        ("subtitle", &l.subtitle),
                        ("privacyPolicyUrl", &l.privacy_policy_url),
                        ("privacyChoicesUrl", &l.privacy_choices_url),
                        ("privacyPolicyText", &l.privacy_policy_text),
                    ]),
                )
            })),
            version: set_fields(&[
                ("copyright", &config.version.copyright),
                ("releaseType", &config.version.release_type),
                ("earliestReleaseDate", &config.version.earliest_release_date),
            ]),
            version_localizations: declared(config.version.localizations.iter().map(|l| {
                (
                    l.locale.clone(),
                    set_fields(&[
                        ("description", &l.description),
                        ("keywords", &l.keywords),
                        ("whatsNew", &l.whats_new),
                        ("promotionalText", &l.promotional_text),
                        ("marketingUrl", &l.marketing_url),
                        ("supportUrl", &l.support_url),
                    ]),
                )
            })),
            review_detail: review_fields(&config.review_detail),
            beta_app_localizations: declared(config.beta_app.localizations.iter().map(|l| {
                (
                    l.locale.clone(),
                    set_fields(&[
                        ("description", &l.description),
                        ("feedbackEmail", &l.feedback_email),
                        ("marketingUrl", &l.marketing_url),
                        ("privacyPolicyUrl", &l.privacy_policy_url),
                        ("tvOsPrivacyPolicy", &l.tv_os_privacy_policy),
                    ]),
                )
            })),
            beta_app_review_detail: review_fields(&config.beta_app_review_detail),
        }
    }

    fn needs_app_info(&self) -> bool {
        !self.categories.is_empty() || !self.app_info_localizations.is_empty()
    }

    fn needs_version(&self) -> bool {
        !self.version.is_empty()
            || !self.version_localizations.is_empty()
            || !self.review_detail.is_empty()
    }
}

fn names(fields: &Map<String, Value>) -> String {
    fields.keys().cloned().collect::<Vec<_>>().join(", ")
}

/// One line per resource `push` would write.
pub fn describe(config: &AppStore) -> Vec<String> {
    let plan = Plan::new(config);
    let mut lines = Vec::new();
    let mut resource = |name: &str, fields: &Map<String, Value>| {
        if !fields.is_empty() {
            lines.push(format!("{name}: {}", names(fields)));
        }
    };
    resource("apps", &plan.app);
    let categories: Map<String, Value> = plan
        .categories
        .iter()
        .map(|(relationship, id)| (relationship.to_string(), json!(id)))
        .collect();
    resource("appInfos", &categories);
    for (locale, fields) in &plan.app_info_localizations {
        resource(&format!("appInfoLocalizations {locale}"), fields);
    }
    resource("appStoreVersions", &plan.version);
    for (locale, fields) in &plan.version_localizations {
        resource(&format!("appStoreVersionLocalizations {locale}"), fields);
    }
    resource("appStoreReviewDetails", &plan.review_detail);
    for (locale, fields) in &plan.beta_app_localizations {
        resource(&format!("betaAppLocalizations {locale}"), fields);
    }
    resource("betaAppReviewDetails", &plan.beta_app_review_detail);
    for image in &config.asset_library.images {
        lines.push(format!(
            "appAssetLibraryImages {}: {}",
            image.reference_name,
            image.file.as_deref().unwrap_or("(no file)")
        ));
    }
    for (locale, group) in placements(config) {
        lines.push(format!(
            "appAssetLibraryPlacements {locale} {} {}: {} image(s)",
            group.placement_type,
            group.placement_group,
            group.images.len()
        ));
    }
    lines
}

fn check_chars(what: &str, value: &Option<String>, limit: usize) -> Result<()> {
    if let Some(value) = value {
        let length = value.trim().chars().count();
        ensure!(
            length <= limit,
            "store.rs: appstore {what} is {length} characters; the limit is {limit}"
        );
    }
    Ok(())
}

fn check_one_of(what: &str, value: &Option<String>, allowed: &[&str]) -> Result<()> {
    if let Some(value) = value {
        ensure!(
            allowed.contains(&value.as_str()),
            "store.rs: appstore {what} is `{value}`; it must be one of {}",
            allowed.join(", ")
        );
    }
    Ok(())
}

fn check_locales<'a>(block: &str, locales: impl Iterator<Item = &'a String>) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for locale in locales {
        ensure!(
            LOCALES.contains(&locale.as_str()),
            "store.rs: appstore {block} locale `{locale}` is not an App Store locale \
             (they look like `en-US`, `ja`, `zh-Hans`)"
        );
        ensure!(
            seen.insert(locale),
            "store.rs: appstore {block} locale `{locale}` is declared twice"
        );
    }
    Ok(())
}

fn validate_assets(config: &AppStore, root: &Path) -> Result<()> {
    let mut declared = std::collections::BTreeSet::new();
    for image in &config.asset_library.images {
        let name = image.reference_name.as_str();
        // The name travels in a `filter[referenceName]` query.
        ensure!(
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)),
            "store.rs: appstore asset_library image `{name}`: reference names may only \
             use letters, digits, `-`, `_` and `.`"
        );
        ensure!(
            declared.insert(name),
            "store.rs: appstore asset_library image `{name}` is declared twice"
        );
        let file = image.file.as_deref().ok_or_else(|| {
            anyhow!("store.rs: appstore asset_library image `{name}` has no file")
        })?;
        files::image(
            root,
            file,
            &format!("appstore asset_library image `{name}`"),
        )?;
        check_one_of(
            &format!("asset_library image `{name}` category"),
            &image.category,
            &[DEFAULT_IMAGE_CATEGORY, "CREATIVE_ASSETS"],
        )?;
    }
    let mut groups = std::collections::BTreeSet::new();
    for (locale, group) in placements(config) {
        let (placement_type, placement_group) = (&group.placement_type, &group.placement_group);
        ensure!(
            groups.insert((locale, placement_type, placement_group)),
            "store.rs: appstore version `{locale}` declares {placement_type} \
             {placement_group} placements twice"
        );
        for name in &group.images {
            ensure!(
                declared.contains(name.as_str()),
                "store.rs: appstore version `{locale}` places image `{name}`, which \
                 asset_library does not declare"
            );
        }
    }
    Ok(())
}

/// Refuse what App Store Connect would reject, before anything is
/// sent. `root` is the directory image paths are relative to.
pub fn validate(config: &AppStore, root: &Path) -> Result<()> {
    validate_assets(config, root)?;
    check_one_of(
        "app content_rights_declaration",
        &config.app.content_rights_declaration,
        &[
            "DOES_NOT_USE_THIRD_PARTY_CONTENT",
            "USES_THIRD_PARTY_CONTENT",
        ],
    )?;
    check_locales(
        "app_info",
        config.app_info.localizations.iter().map(|l| &l.locale),
    )?;
    for l in &config.app_info.localizations {
        let locale = &l.locale;
        check_chars(&format!("app_info `{locale}` name"), &l.name, NAME_LIMIT)?;
        if let Some(name) = &l.name {
            ensure!(
                name.trim().chars().count() >= 2,
                "store.rs: appstore app_info `{locale}` name needs at least 2 characters"
            );
        }
        check_chars(
            &format!("app_info `{locale}` subtitle"),
            &l.subtitle,
            SUBTITLE_LIMIT,
        )?;
    }
    check_one_of(
        "version release_type",
        &config.version.release_type,
        &["MANUAL", "AFTER_APPROVAL", "SCHEDULED"],
    )?;
    check_locales(
        "version",
        config.version.localizations.iter().map(|l| &l.locale),
    )?;
    for l in &config.version.localizations {
        let locale = &l.locale;
        check_chars(
            &format!("version `{locale}` description"),
            &l.description,
            DESCRIPTION_LIMIT,
        )?;
        check_chars(
            &format!("version `{locale}` keywords"),
            &l.keywords,
            KEYWORDS_LIMIT,
        )?;
        check_chars(
            &format!("version `{locale}` whats_new"),
            &l.whats_new,
            WHATS_NEW_LIMIT,
        )?;
        check_chars(
            &format!("version `{locale}` promotional_text"),
            &l.promotional_text,
            PROMOTIONAL_TEXT_LIMIT,
        )?;
    }
    check_chars(
        "review_detail notes",
        &config.review_detail.notes,
        REVIEW_NOTES_LIMIT,
    )?;
    check_locales(
        "beta_app",
        config.beta_app.localizations.iter().map(|l| &l.locale),
    )?;
    check_chars(
        "beta_app_review_detail notes",
        &config.beta_app_review_detail.notes,
        REVIEW_NOTES_LIMIT,
    )?;
    Ok(())
}

fn list(api: &impl Api, path: &str) -> Result<Vec<Value>> {
    Ok(api
        .get(path)?
        .and_then(|json| json.get("data")?.as_array().cloned())
        .unwrap_or_default())
}

fn id_of(resource: &Value) -> Option<&str> {
    resource.get("id")?.as_str()
}

fn attribute<'a>(resource: &'a Value, name: &str) -> Option<&'a str> {
    resource.get("attributes")?.get(name)?.as_str()
}

fn linkage(resource_type: &str, id: &str) -> Value {
    json!({ "data": { "type": resource_type, "id": id } })
}

fn patch(
    api: &impl Api,
    resource_type: &str,
    id: &str,
    attributes: Map<String, Value>,
) -> Result<()> {
    api.send(
        "PATCH",
        &format!("/v1/{resource_type}/{id}"),
        json!({ "data": { "type": resource_type, "id": id, "attributes": attributes } }),
    )
    .map(|_| ())
}

/// A related parent resource: `(relationship, resource type, id)`.
type Parent<'a> = (&'a str, &'a str, &'a str);

/// Update each declared locale's localization, creating the ones the
/// parent doesn't have yet. Undeclared locales are left alone.
fn upsert_localizations(
    api: &impl Api,
    done: &mut Vec<String>,
    resource_type: &str,
    list_path: &str,
    parent: Parent,
    declared: Vec<(String, Map<String, Value>)>,
) -> Result<()> {
    if declared.is_empty() {
        return Ok(());
    }
    let existing = list(api, list_path)?;
    for (locale, mut attributes) in declared {
        let found = existing
            .iter()
            .find(|l| attribute(l, "locale") == Some(locale.as_str()))
            .and_then(id_of);
        let fields = names(&attributes);
        match found {
            Some(id) => {
                done.push(format!("{resource_type} {locale} (updated): {fields}"));
                patch(api, resource_type, id, attributes)?;
            }
            None => {
                done.push(format!("{resource_type} {locale} (created): {fields}"));
                attributes.insert("locale".into(), json!(locale));
                let (relationship, parent_type, parent_id) = parent;
                api.send(
                    "POST",
                    &format!("/v1/{resource_type}"),
                    json!({ "data": {
                        "type": resource_type,
                        "attributes": attributes,
                        "relationships": { relationship: linkage(parent_type, parent_id) },
                    }}),
                )?;
            }
        }
    }
    Ok(())
}

/// The first resource whose state (read from the first of
/// `state_attributes` it carries) accepts changes.
fn editable<'a>(
    resources: &'a [Value],
    state_attributes: &[&str],
    states: &[&str],
) -> Option<&'a str> {
    resources
        .iter()
        .find(|resource| {
            state_attributes
                .iter()
                .find_map(|name| attribute(resource, name))
                .is_some_and(|state| states.contains(&state))
        })
        .and_then(id_of)
}

fn editable_version(
    api: &impl Api,
    app_id: &str,
    options: &PushOptions,
) -> Result<(String, String)> {
    let versions = list(
        api,
        &format!("/v1/apps/{app_id}/appStoreVersions?filter[platform]=IOS&limit=200"),
    )?;
    // `appVersionState` replaced `appStoreState`; older responses
    // carry only the latter.
    if let Some(id) = editable(
        &versions,
        &["appVersionState", "appStoreState"],
        &EDITABLE_VERSION_STATES,
    ) {
        let version = versions
            .iter()
            .find(|v| id_of(v) == Some(id))
            .and_then(|v| attribute(v, "versionString"))
            .unwrap_or("?");
        return Ok((id.to_string(), version.to_string()));
    }
    let Some(version) = options.create_version else {
        bail!(
            "no App Store version is being prepared, and version metadata can only be \
             written to one.\n\
             Fix: create the next version in App Store Connect, or re-run with \
             `--create-version` to create it from whisker.rs's version."
        );
    };
    let created = api.send(
        "POST",
        "/v1/appStoreVersions",
        json!({ "data": {
            "type": "appStoreVersions",
            "attributes": { "platform": "IOS", "versionString": version },
            "relationships": { "app": linkage("apps", app_id) },
        }}),
    )?;
    let id = created
        .pointer("/data/id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("App Store Connect returned no id for the new version"))?;
    Ok((id.to_string(), version.to_string()))
}

/// The library image for `image`, uploading the file unless the
/// library already holds it under the same reference name.
///
/// An image's bytes can't be changed once committed, so a changed
/// file is uploaded as a new image; the old one stays in the library.
fn ensure_image(
    api: &impl Api,
    done: &mut Vec<String>,
    library_id: &str,
    image: &AssetLibraryImage,
    root: &Path,
) -> Result<String> {
    let name = image.reference_name.as_str();
    let (path, _) = files::image(root, image.file.as_deref().unwrap_or_default(), name)?;
    let size = path
        .metadata()
        .with_context(|| format!("read {}", path.display()))?
        .len();
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("{} has no UTF-8 file name", path.display()))?;

    let existing = list(
        api,
        &format!(
            "/v1/appAssetLibraries/{library_id}/images?filter[referenceName]={name}&limit=200"
        ),
    )?;
    let reusable = existing.iter().find(|candidate| {
        attribute(candidate, "referenceName") == Some(name)
            && attribute(candidate, "fileName") == Some(file_name)
            && candidate
                .pointer("/attributes/fileSize")
                .and_then(Value::as_u64)
                == Some(size)
            && !["AWAITING_UPLOAD", "FAILED", "ARCHIVED"]
                .contains(&attribute(candidate, "state").unwrap_or_default())
    });
    if let Some(id) = reusable.and_then(id_of) {
        done.push(format!("appAssetLibraryImages {name}: unchanged"));
        return Ok(id.to_string());
    }

    done.push(format!(
        "appAssetLibraryImages {name}: uploaded {file_name}"
    ));
    let reserved = api.send(
        "POST",
        "/v1/appAssetLibraryImages",
        json!({ "data": {
            "type": "appAssetLibraryImages",
            "attributes": {
                "fileName": file_name,
                "fileSize": size,
                "category": category(image),
                "referenceName": name,
            },
            "relationships": { "assetLibrary": linkage("appAssetLibraries", library_id) },
        }}),
    )?;
    let id = reserved
        .pointer("/data/id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("App Store Connect returned no id for image `{name}`"))?
        .to_string();
    let operations: Vec<UploadOperation> = reserved
        .pointer("/data/attributes/uploadOperations")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .context("parse uploadOperations")?
        .unwrap_or_default();
    ensure!(
        !operations.is_empty(),
        "App Store Connect returned no upload operations for image `{name}`"
    );
    api.upload(&path, size, &operations)?;
    let mut uploaded = Map::new();
    uploaded.insert("uploaded".into(), json!(true));
    patch(api, "appAssetLibraryImages", &id, uploaded)?;
    await_processing(api, &id, name)?;
    Ok(id)
}

/// Wait until App Store Connect has matched an uploaded image against
/// its specifications. A file whose dimensions match none fails here;
/// placing it anyway would leave a placement that never appears.
fn await_processing(api: &impl Api, id: &str, name: &str) -> Result<()> {
    for _ in 0..PROCESSING_POLLS {
        let image = api
            .get(&format!("/v1/appAssetLibraryImages/{id}"))?
            .and_then(|json| json.get("data").cloned())
            .unwrap_or(Value::Null);
        match attribute(&image, "state").unwrap_or_default() {
            "AWAITING_UPLOAD" | "UPLOAD_COMPLETE" | "" => api.pause(),
            "FAILED" => bail!(
                "App Store Connect rejected image `{name}`: {}\n\
                 Fix: its dimensions must match one of App Store Connect's image \
                 specifications exactly. The failed image stays in the asset library \
                 until deleted there.",
                image
                    .pointer("/attributes/stateDetails")
                    .filter(|details| !details.is_null())
                    .map(Value::to_string)
                    .unwrap_or_else(|| "(no detail given)".to_string())
            ),
            _ => return Ok(()),
        }
    }
    bail!("image `{name}` was still processing after five minutes — re-run the push")
}

/// Make one placement group on a localization show exactly `wanted`
/// (library image ids), in order. A placement can't be edited, so a
/// group that differs is deleted and recreated.
fn sync_placements(
    api: &impl Api,
    done: &mut Vec<String>,
    localization_id: &str,
    locale: &str,
    group: &Placements,
    wanted: &[&str],
) -> Result<()> {
    let (placement_type, placement_group) = (&group.placement_type, &group.placement_group);
    let label = format!("appAssetLibraryPlacements {locale} {placement_type} {placement_group}");
    let existing = list(
        api,
        &format!(
            "/v1/appStoreVersionLocalizations/{localization_id}/placements\
             ?filter[placementType]={placement_type}&filter[placementGroup]={placement_group}\
             &sort=placementGroupPosition&include=image&limit=200"
        ),
    )?;
    let shown: Vec<&str> = existing
        .iter()
        .filter_map(|p| p.pointer("/relationships/image/data/id")?.as_str())
        .collect();
    if shown == wanted && shown.len() == existing.len() {
        done.push(format!("{label}: unchanged"));
        return Ok(());
    }
    done.push(format!(
        "{label}: replaced {} with {}",
        existing.len(),
        wanted.len()
    ));
    for placement in existing.iter().filter_map(id_of) {
        api.send(
            "DELETE",
            &format!("/v1/appAssetLibraryPlacements/{placement}"),
            Value::Null,
        )?;
    }
    let localization = linkage("appStoreVersionLocalizations", localization_id);
    let mut created = Vec::new();
    for image in wanted {
        let placement = api.send(
            "POST",
            "/v1/appAssetLibraryPlacements",
            json!({ "data": {
                "type": "appAssetLibraryPlacements",
                "attributes": {
                    "placementType": placement_type,
                    "placementGroup": placement_group,
                },
                "relationships": {
                    "image": linkage("appAssetLibraryImages", image),
                    "appStoreVersionLocalization": localization,
                },
            }}),
        )?;
        let id = placement
            .pointer("/data/id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("App Store Connect returned no id for a new placement"))?;
        created.push(json!({ "type": "appAssetLibraryPlacements", "id": id }));
    }
    if created.len() > 1 {
        api.send(
            "POST",
            "/v1/appAssetLibraryPlacementOrderingRequests",
            json!({ "data": {
                "type": "appAssetLibraryPlacementOrderingRequests",
                "attributes": { "placementGroup": placement_group },
                "relationships": {
                    "orderedPlacements": { "data": created },
                    "appStoreVersionLocalization": localization,
                },
            }}),
        )?;
    }
    Ok(())
}

/// Send everything `store push` owns in `config`. Returns one line
/// per resource written. `root` is the directory image paths are
/// relative to.
pub fn push(
    api: &impl Api,
    bundle_id: &str,
    config: &AppStore,
    root: &Path,
    options: &PushOptions,
) -> Result<Vec<String>> {
    validate(config, root)?;
    let plan = Plan::new(config);
    if describe(config).is_empty() {
        bail!("store.rs declares nothing `store push appstore` sends");
    }

    let apps = api
        .get(&format!(
            "/v1/apps?filter[bundleId]={bundle_id}&fields[apps]=bundleId"
        ))?
        .unwrap_or(Value::Null);
    let app_id = asc::app_id_in(&apps, bundle_id).ok_or_else(|| {
        anyhow!(
            "no app with bundle id {bundle_id} in App Store Connect. The API cannot create \
             apps — add it at https://appstoreconnect.apple.com/apps (＋ → New App)."
        )
    })?;
    let app_info_id = if plan.needs_app_info() {
        let infos = list(api, &format!("/v1/apps/{app_id}/appInfos"))?;
        let id = editable(&infos, &["state"], &EDITABLE_APP_INFO_STATES).ok_or_else(|| {
            anyhow!(
                "the app's information (name, subtitle, categories) is not editable right \
                     now — App Store Connect only accepts changes to it while a version is \
                     being prepared."
            )
        })?;
        Some(id.to_string())
    } else {
        None
    };
    let has_placements = placements(config).next().is_some();
    let version = if plan.needs_version() || has_placements {
        Some(editable_version(api, &app_id, options)?)
    } else {
        None
    };
    let library_id = if config.asset_library.images.is_empty() {
        None
    } else {
        let id = api
            .get(&format!("/v1/apps/{app_id}/assetLibrary"))?
            .and_then(|json| json.pointer("/data/id")?.as_str().map(str::to_string))
            .ok_or_else(|| anyhow!("App Store Connect has no asset library for this app"))?;
        Some(id)
    };

    let mut done = Vec::new();
    if !plan.app.is_empty() {
        done.push(format!("apps: {}", names(&plan.app)));
        patch(api, "apps", &app_id, plan.app)?;
    }
    if let Some(info_id) = &app_info_id {
        if !plan.categories.is_empty() {
            let relationships: Map<String, Value> = plan
                .categories
                .iter()
                .map(|(relationship, id)| (relationship.to_string(), linkage("appCategories", id)))
                .collect();
            done.push(format!("appInfos: {}", names(&relationships)));
            api.send(
                "PATCH",
                &format!("/v1/appInfos/{info_id}"),
                json!({ "data": {
                    "type": "appInfos",
                    "id": info_id,
                    "relationships": relationships,
                }}),
            )?;
        }
        upsert_localizations(
            api,
            &mut done,
            "appInfoLocalizations",
            &format!("/v1/appInfos/{info_id}/appInfoLocalizations?limit=200"),
            ("appInfo", "appInfos", info_id),
            plan.app_info_localizations,
        )?;
    }
    if let Some((version_id, version_string)) = &version {
        done.push(format!("writing to version {version_string}"));
        if !plan.version.is_empty() {
            done.push(format!("appStoreVersions: {}", names(&plan.version)));
            patch(api, "appStoreVersions", version_id, plan.version)?;
        }
        upsert_localizations(
            api,
            &mut done,
            "appStoreVersionLocalizations",
            &format!("/v1/appStoreVersions/{version_id}/appStoreVersionLocalizations?limit=200"),
            ("appStoreVersion", "appStoreVersions", version_id),
            plan.version_localizations,
        )?;
        let mut images = std::collections::BTreeMap::new();
        if let Some(library_id) = &library_id {
            for image in &config.asset_library.images {
                let id = ensure_image(api, &mut done, library_id, image, root)?;
                images.insert(image.reference_name.as_str(), id);
            }
        }
        if has_placements {
            let localizations_path =
                format!("/v1/appStoreVersions/{version_id}/appStoreVersionLocalizations?limit=200");
            let mut localizations = list(api, &localizations_path)?;
            for (locale, group) in placements(config) {
                let existing = localizations
                    .iter()
                    .find(|l| attribute(l, "locale") == Some(locale))
                    .and_then(id_of)
                    .map(str::to_string);
                let localization_id = match existing {
                    Some(id) => id,
                    None => {
                        done.push(format!("appStoreVersionLocalizations {locale} (created)"));
                        let created = api.send(
                            "POST",
                            "/v1/appStoreVersionLocalizations",
                            json!({ "data": {
                                "type": "appStoreVersionLocalizations",
                                "attributes": { "locale": locale },
                                "relationships": {
                                    "appStoreVersion": linkage("appStoreVersions", version_id),
                                },
                            }}),
                        )?;
                        localizations = list(api, &localizations_path)?;
                        created
                            .pointer("/data/id")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                anyhow!("App Store Connect returned no id for locale {locale}")
                            })?
                            .to_string()
                    }
                };
                let wanted: Vec<&str> = group
                    .images
                    .iter()
                    .filter_map(|name| images.get(name.as_str()).map(String::as_str))
                    .collect();
                sync_placements(api, &mut done, &localization_id, locale, group, &wanted)?;
            }
        }
        if !plan.review_detail.is_empty() {
            done.push(format!(
                "appStoreReviewDetails: {}",
                names(&plan.review_detail)
            ));
            let existing = api
                .get(&format!(
                    "/v1/appStoreVersions/{version_id}/appStoreReviewDetail"
                ))?
                .and_then(|json| json.pointer("/data/id")?.as_str().map(str::to_string));
            match existing {
                Some(id) => patch(api, "appStoreReviewDetails", &id, plan.review_detail)?,
                None => {
                    api.send(
                        "POST",
                        "/v1/appStoreReviewDetails",
                        json!({ "data": {
                            "type": "appStoreReviewDetails",
                            "attributes": plan.review_detail,
                            "relationships": {
                                "appStoreVersion": linkage("appStoreVersions", version_id),
                            },
                        }}),
                    )?;
                }
            }
        }
    }
    if version.is_none() {
        if let Some(library_id) = &library_id {
            for image in &config.asset_library.images {
                ensure_image(api, &mut done, library_id, image, root)?;
            }
        }
    }
    upsert_localizations(
        api,
        &mut done,
        "betaAppLocalizations",
        &format!("/v1/apps/{app_id}/betaAppLocalizations?limit=200"),
        ("app", "apps", &app_id),
        plan.beta_app_localizations,
    )?;
    if !plan.beta_app_review_detail.is_empty() {
        done.push(format!(
            "betaAppReviewDetails: {}",
            names(&plan.beta_app_review_detail)
        ));
        let id = api
            .get(&format!("/v1/apps/{app_id}/betaAppReviewDetail"))?
            .and_then(|json| json.pointer("/data/id")?.as_str().map(str::to_string))
            .ok_or_else(|| anyhow!("App Store Connect has no beta review detail for this app"))?;
        patch(
            api,
            "betaAppReviewDetails",
            &id,
            plan.beta_app_review_detail,
        )?;
    }
    Ok(done)
}

fn owned(resource: &Value, name: &str) -> Option<String> {
    attribute(resource, name)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn review_detail_from(resource: &Value) -> ReviewDetail {
    ReviewDetail {
        contact_first_name: owned(resource, "contactFirstName"),
        contact_last_name: owned(resource, "contactLastName"),
        contact_phone: owned(resource, "contactPhone"),
        contact_email: owned(resource, "contactEmail"),
        demo_account_required: resource
            .pointer("/attributes/demoAccountRequired")
            .and_then(Value::as_bool),
        notes: owned(resource, "notes"),
    }
}

/// The resource `pull` should read: the one being edited when there
/// is one — that is what a later `push` writes to — else the live
/// one. The list's order is not specified, so "live" is found by
/// state, not position.
fn current<'a>(
    resources: &'a [Value],
    state_attributes: &[&str],
    states: &[&str],
) -> Option<&'a Value> {
    let by_id = |id: &str| resources.iter().find(|r| id_of(r) == Some(id));
    editable(resources, state_attributes, states)
        .and_then(by_id)
        .or_else(|| {
            editable(resources, state_attributes, &["READY_FOR_DISTRIBUTION"]).and_then(by_id)
        })
        .or(resources.first())
}

/// Read what App Store Connect holds for everything `push` writes,
/// plus a note per choice that affects what was read.
pub fn pull(api: &impl Api, bundle_id: &str) -> Result<(AppStore, Vec<String>)> {
    let apps = api
        .get(&format!(
            "/v1/apps?filter[bundleId]={bundle_id}&fields[apps]=bundleId"
        ))?
        .unwrap_or(Value::Null);
    let app_id = asc::app_id_in(&apps, bundle_id)
        .ok_or_else(|| anyhow!("no app with bundle id {bundle_id} in App Store Connect"))?;
    let mut config = AppStore::default();
    let mut notes = Vec::new();

    if let Some(app) = api.get(&format!("/v1/apps/{app_id}"))? {
        let app = app.get("data").cloned().unwrap_or(Value::Null);
        config.app.content_rights_declaration = owned(&app, "contentRightsDeclaration");
        config.app.primary_locale = owned(&app, "primaryLocale");
    }

    let infos = list(api, &format!("/v1/apps/{app_id}/appInfos"))?;
    if let Some(info) = current(&infos, &["state"], &EDITABLE_APP_INFO_STATES) {
        let info_id = id_of(info).unwrap_or_default();
        notes.push(format!(
            "app_info read from the app info in state {}",
            attribute(info, "state").unwrap_or("?")
        ));
        let category = |relationship: &str| -> Result<Option<String>> {
            Ok(api
                .get(&format!("/v1/appInfos/{info_id}/{relationship}"))?
                .and_then(|json| json.pointer("/data/id")?.as_str().map(str::to_string)))
        };
        config.app_info.primary_category = category("primaryCategory")?;
        config.app_info.primary_subcategory_one = category("primarySubcategoryOne")?;
        config.app_info.primary_subcategory_two = category("primarySubcategoryTwo")?;
        config.app_info.secondary_category = category("secondaryCategory")?;
        config.app_info.secondary_subcategory_one = category("secondarySubcategoryOne")?;
        config.app_info.secondary_subcategory_two = category("secondarySubcategoryTwo")?;
        config.app_info.localizations = list(
            api,
            &format!("/v1/appInfos/{info_id}/appInfoLocalizations?limit=200"),
        )?
        .iter()
        .map(|l| AppInfoLocalization {
            locale: owned(l, "locale").unwrap_or_default(),
            name: owned(l, "name"),
            subtitle: owned(l, "subtitle"),
            privacy_policy_url: owned(l, "privacyPolicyUrl"),
            privacy_choices_url: owned(l, "privacyChoicesUrl"),
            privacy_policy_text: owned(l, "privacyPolicyText"),
        })
        .collect();
    }

    let versions = list(
        api,
        &format!("/v1/apps/{app_id}/appStoreVersions?filter[platform]=IOS&limit=200"),
    )?;
    let state_attributes = ["appVersionState", "appStoreState"];
    if let Some(version) = current(&versions, &state_attributes, &EDITABLE_VERSION_STATES) {
        let version_id = id_of(version).unwrap_or_default();
        notes.push(format!(
            "version read from {} ({})",
            attribute(version, "versionString").unwrap_or("?"),
            state_attributes
                .iter()
                .find_map(|name| attribute(version, name))
                .unwrap_or("?")
        ));
        config.version.copyright = owned(version, "copyright");
        config.version.release_type = owned(version, "releaseType");
        config.version.earliest_release_date = owned(version, "earliestReleaseDate");
        config.version.localizations = list(
            api,
            &format!("/v1/appStoreVersions/{version_id}/appStoreVersionLocalizations?limit=200"),
        )?
        .iter()
        .map(|l| AppStoreVersionLocalization {
            locale: owned(l, "locale").unwrap_or_default(),
            description: owned(l, "description"),
            keywords: owned(l, "keywords"),
            whats_new: owned(l, "whatsNew"),
            promotional_text: owned(l, "promotionalText"),
            marketing_url: owned(l, "marketingUrl"),
            support_url: owned(l, "supportUrl"),
            placements: Vec::new(),
        })
        .collect();
        if let Some(detail) = api
            .get(&format!(
                "/v1/appStoreVersions/{version_id}/appStoreReviewDetail"
            ))?
            .and_then(|json| json.get("data").cloned())
        {
            config.review_detail = review_detail_from(&detail);
        }
    }

    config.beta_app.localizations = list(
        api,
        &format!("/v1/apps/{app_id}/betaAppLocalizations?limit=200"),
    )?
    .iter()
    .map(|l| BetaAppLocalization {
        locale: owned(l, "locale").unwrap_or_default(),
        description: owned(l, "description"),
        feedback_email: owned(l, "feedbackEmail"),
        marketing_url: owned(l, "marketingUrl"),
        privacy_policy_url: owned(l, "privacyPolicyUrl"),
        tv_os_privacy_policy: owned(l, "tvOsPrivacyPolicy"),
    })
    .collect();
    if let Some(detail) = api
        .get(&format!("/v1/apps/{app_id}/betaAppReviewDetail"))?
        .and_then(|json| json.get("data").cloned())
    {
        config.beta_app_review_detail = review_detail_from(&detail);
    }
    Ok((config, notes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Fake {
        responses: BTreeMap<&'static str, Value>,
        sent: RefCell<Vec<(String, String, Value)>>,
        uploads: RefCell<Vec<String>>,
    }

    impl Api for Fake {
        fn get(&self, path: &str) -> Result<Option<Value>> {
            Ok(self.responses.get(path).cloned())
        }

        fn send(&self, method: &str, path: &str, body: Value) -> Result<Value> {
            self.sent
                .borrow_mut()
                .push((method.to_string(), path.to_string(), body));
            let id = format!("new{}", self.sent.borrow().len());
            Ok(
                json!({ "data": { "id": id, "attributes": { "uploadOperations": [
                    { "method": "PUT", "url": "https://example.invalid/part" },
                ]}}}),
            )
        }

        fn pause(&self) {}

        fn upload(&self, file: &Path, size: u64, operations: &[UploadOperation]) -> Result<()> {
            assert_eq!(operations.len(), 1);
            self.uploads.borrow_mut().push(format!(
                "{} ({size} bytes)",
                file.file_name().unwrap().to_string_lossy()
            ));
            Ok(())
        }
    }

    const APPS: &str = "/v1/apps?filter[bundleId]=com.example.app&fields[apps]=bundleId";
    const VERSIONS: &str = "/v1/apps/1/appStoreVersions?filter[platform]=IOS&limit=200";

    fn fake(responses: &[(&'static str, Value)]) -> Fake {
        let mut all = BTreeMap::from([(
            APPS,
            json!({ "data": [{ "id": "1", "attributes": { "bundleId": "com.example.app" } }] }),
        )]);
        all.extend(responses.iter().cloned());
        Fake {
            responses: all,
            ..Fake::default()
        }
    }

    fn sent(fake: &Fake) -> Vec<(String, String)> {
        fake.sent
            .borrow()
            .iter()
            .map(|(method, path, _)| (method.clone(), path.clone()))
            .collect()
    }

    const NO_CREATE: PushOptions = PushOptions {
        create_version: None,
    };

    #[test]
    fn version_metadata_goes_to_the_editable_version_updating_or_creating_locales() {
        let mut config = AppStore::default();
        config.version(|v| {
            v.copyright("2026 Example");
            v.locale("ja", |l| {
                l.description("説明");
            });
            v.locale("en-US", |l| {
                l.description("Description").keywords("a,b");
            });
        });
        let api = fake(&[
            (
                VERSIONS,
                json!({ "data": [
                    { "id": "live", "attributes": { "appVersionState": "READY_FOR_DISTRIBUTION", "versionString": "1.0" } },
                    { "id": "next", "attributes": { "appVersionState": "PREPARE_FOR_SUBMISSION", "versionString": "1.1" } },
                ]}),
            ),
            (
                "/v1/appStoreVersions/next/appStoreVersionLocalizations?limit=200",
                json!({ "data": [{ "id": "loc-ja", "attributes": { "locale": "ja" } }] }),
            ),
        ]);
        let done = push(&api, "com.example.app", &config, Path::new("."), &NO_CREATE).unwrap();

        assert_eq!(
            sent(&api),
            vec![
                ("PATCH".to_string(), "/v1/appStoreVersions/next".to_string()),
                (
                    "PATCH".to_string(),
                    "/v1/appStoreVersionLocalizations/loc-ja".to_string()
                ),
                (
                    "POST".to_string(),
                    "/v1/appStoreVersionLocalizations".to_string()
                ),
            ]
        );
        let created = &api.sent.borrow()[2].2;
        assert_eq!(
            created["data"]["attributes"],
            json!({ "description": "Description", "keywords": "a,b", "locale": "en-US" })
        );
        assert_eq!(
            created["data"]["relationships"]["appStoreVersion"]["data"]["id"],
            "next"
        );
        assert!(done.contains(&"writing to version 1.1".to_string()));
    }

    #[test]
    fn nothing_is_written_when_no_version_is_editable() {
        let mut config = AppStore::default();
        config
            .app(|a| {
                a.primary_locale("ja");
            })
            .version(|v| {
                v.copyright("2026 Example");
            });
        let live = json!({ "data": [
            { "id": "live", "attributes": { "appVersionState": "READY_FOR_DISTRIBUTION", "versionString": "1.0" } },
        ]});

        let api = fake(&[(VERSIONS, live.clone())]);
        let err = push(&api, "com.example.app", &config, Path::new("."), &NO_CREATE)
            .unwrap_err()
            .to_string();
        assert!(err.contains("--create-version"), "{err}");
        assert!(sent(&api).is_empty(), "the apps PATCH must not have run");

        let api = fake(&[(VERSIONS, live)]);
        let options = PushOptions {
            create_version: Some("1.1"),
        };
        push(&api, "com.example.app", &config, Path::new("."), &options).unwrap();
        let calls = api.sent.borrow();
        assert_eq!(calls[0].1, "/v1/appStoreVersions");
        assert_eq!(calls[0].2["data"]["attributes"]["versionString"], "1.1");
        assert_eq!(calls[2].1, "/v1/appStoreVersions/new1");
    }

    #[test]
    fn app_info_needs_an_editable_app_info_and_sends_categories_as_relationships() {
        let mut config = AppStore::default();
        config.app_info(|i| {
            i.primary_category("BOOKS");
            i.locale("ja", |l| {
                l.name("アプリ");
            });
        });

        let api = fake(&[(
            "/v1/apps/1/appInfos",
            json!({ "data": [{ "id": "i1", "attributes": { "state": "READY_FOR_DISTRIBUTION" } }] }),
        )]);
        let err = push(&api, "com.example.app", &config, Path::new("."), &NO_CREATE)
            .unwrap_err()
            .to_string();
        assert!(err.contains("not editable"), "{err}");
        assert!(sent(&api).is_empty());

        let api = fake(&[(
            "/v1/apps/1/appInfos",
            json!({ "data": [
                { "id": "i1", "attributes": { "state": "READY_FOR_DISTRIBUTION" } },
                { "id": "i2", "attributes": { "state": "PREPARE_FOR_SUBMISSION" } },
            ]}),
        )]);
        push(&api, "com.example.app", &config, Path::new("."), &NO_CREATE).unwrap();
        let calls = api.sent.borrow();
        assert_eq!(calls[0].1, "/v1/appInfos/i2");
        assert_eq!(
            calls[0].2["data"]["relationships"]["primaryCategory"]["data"],
            json!({ "type": "appCategories", "id": "BOOKS" })
        );
        assert_eq!(calls[1].1, "/v1/appInfoLocalizations");
    }

    #[test]
    fn review_and_beta_details_update_in_place() {
        let mut config = AppStore::default();
        config
            .beta_app(|b| {
                b.locale("ja", |l| {
                    l.feedback_email("beta@example.com");
                });
            })
            .beta_app_review_detail(|r| {
                r.demo_account_required(false).notes("メモ");
            });
        let api = fake(&[
            (
                "/v1/apps/1/betaAppLocalizations?limit=200",
                json!({ "data": [{ "id": "b-ja", "attributes": { "locale": "ja" } }] }),
            ),
            (
                "/v1/apps/1/betaAppReviewDetail",
                json!({ "data": { "id": "1" } }),
            ),
        ]);
        push(&api, "com.example.app", &config, Path::new("."), &NO_CREATE).unwrap();
        let calls = api.sent.borrow();
        assert_eq!(calls[0].1, "/v1/betaAppLocalizations/b-ja");
        assert_eq!(calls[1].1, "/v1/betaAppReviewDetails/1");
        assert_eq!(
            calls[1].2["data"]["attributes"],
            json!({ "demoAccountRequired": false, "notes": "メモ" })
        );
    }

    #[test]
    fn pull_prefers_the_version_being_prepared_and_never_writes() {
        let api = fake(&[
            (
                "/v1/apps/1",
                json!({ "data": { "id": "1", "attributes": { "primaryLocale": "ja" } } }),
            ),
            (
                "/v1/apps/1/appInfos",
                json!({ "data": [{ "id": "i1", "attributes": { "state": "READY_FOR_DISTRIBUTION" } }] }),
            ),
            (
                "/v1/appInfos/i1/primaryCategory",
                json!({ "data": { "type": "appCategories", "id": "BOOKS" } }),
            ),
            (
                "/v1/appInfos/i1/appInfoLocalizations?limit=200",
                json!({ "data": [{ "id": "l1", "attributes": { "locale": "ja", "name": "アプリ", "subtitle": null } }] }),
            ),
            (
                VERSIONS,
                json!({ "data": [
                    { "id": "live", "attributes": { "appVersionState": "READY_FOR_DISTRIBUTION", "versionString": "1.0", "copyright": "old" } },
                    { "id": "next", "attributes": { "appVersionState": "PREPARE_FOR_SUBMISSION", "versionString": "1.1", "copyright": "2026 Example" } },
                ]}),
            ),
            (
                "/v1/appStoreVersions/next/appStoreVersionLocalizations?limit=200",
                json!({ "data": [{ "id": "v1", "attributes": { "locale": "ja", "description": "説明", "keywords": "" } }] }),
            ),
            (
                "/v1/appStoreVersions/next/appStoreReviewDetail",
                json!({ "data": { "id": "r1", "attributes": {
                    "contactEmail": "review@example.com",
                    "demoAccountRequired": true,
                    "demoAccountName": "user",
                    "demoAccountPassword": "secret",
                }}}),
            ),
        ]);
        let (pulled, notes) = pull(&api, "com.example.app").unwrap();

        let mut expected = AppStore::default();
        expected
            .app(|a| {
                a.primary_locale("ja");
            })
            .app_info(|i| {
                i.primary_category("BOOKS");
                i.locale("ja", |l| {
                    l.name("アプリ");
                });
            })
            .version(|v| {
                v.copyright("2026 Example");
                v.locale("ja", |l| {
                    l.description("説明");
                });
            })
            .review_detail(|r| {
                r.contact_email("review@example.com")
                    .demo_account_required(true);
            });
        assert_eq!(pulled, expected);
        assert!(
            notes.contains(&"version read from 1.1 (PREPARE_FOR_SUBMISSION)".to_string()),
            "{notes:?}"
        );
        assert!(sent(&api).is_empty(), "pull must not write");
        // The demo account's credentials must not reach store.rs.
        assert!(!crate::render::appstore(&pulled).contains("secret"));
    }

    #[test]
    fn without_a_version_in_preparation_pull_reads_the_live_one_wherever_it_is_listed() {
        let api = fake(&[(
            VERSIONS,
            json!({ "data": [
                { "id": "old", "attributes": { "appVersionState": "REPLACED_WITH_NEW_VERSION", "versionString": "1.0", "copyright": "old" } },
                { "id": "live", "attributes": { "appVersionState": "READY_FOR_DISTRIBUTION", "versionString": "1.1", "copyright": "live" } },
            ]}),
        )]);
        let (pulled, notes) = pull(&api, "com.example.app").unwrap();
        assert_eq!(pulled.version.copyright.as_deref(), Some("live"));
        assert!(
            notes.contains(&"version read from 1.1 (READY_FOR_DISTRIBUTION)".to_string()),
            "{notes:?}"
        );
    }

    #[test]
    fn limits_count_characters_and_reject_values_outside_an_enum() {
        // Taken from a live listing: 82 characters, 184 bytes.
        let thai = "การ์ตูน,PDF,CBZ,ชั้นหนังสือ,เว็บตูน,Dropbox,OneDrive,อีบุ๊ก,ฟรี,เบราว์เซอร์,คอมมิค";
        let mut config = AppStore::default();
        config.version(|v| {
            v.locale("th", |l| {
                l.keywords(thai);
            });
        });
        validate(&config, Path::new(".")).unwrap();

        let mut config = AppStore::default();
        config.version(|v| {
            v.locale("ja", |l| {
                l.keywords("あ".repeat(101));
            });
        });
        let err = validate(&config, Path::new(".")).unwrap_err().to_string();
        assert!(err.contains("keywords is 101 characters"), "{err}");

        let mut config = AppStore::default();
        config.app_info(|i| {
            i.locale("ja", |l| {
                l.name("あ");
            });
        });
        assert!(
            validate(&config, Path::new("."))
                .unwrap_err()
                .to_string()
                .contains("at least 2")
        );

        let mut config = AppStore::default();
        config.version(|v| {
            v.release_type("LATER");
        });
        assert!(
            validate(&config, Path::new("."))
                .unwrap_err()
                .to_string()
                .contains("MANUAL, AFTER_APPROVAL, SCHEDULED")
        );
    }

    #[test]
    fn a_play_style_locale_is_refused_before_anything_is_sent() {
        let mut config = AppStore::default();
        config.version(|v| {
            v.locale("ja-JP", |l| {
                l.description("説明");
            });
        });
        let err = validate(&config, Path::new(".")).unwrap_err().to_string();
        assert!(err.contains("`ja-JP` is not an App Store locale"), "{err}");
    }

    #[test]
    fn describe_omits_what_submit_sends_and_what_is_unset() {
        let mut config = AppStore::default();
        config
            .version(|v| {
                v.locale("ja", |l| {
                    l.description("説明");
                });
                v.locale("fr-FR", |_| {});
            })
            .beta_build(|b| {
                b.locale("ja", |l| {
                    l.whats_new("修正");
                });
            });
        assert_eq!(
            describe(&config),
            vec!["appStoreVersionLocalizations ja: description"]
        );
    }

    const LOCALIZATIONS: &str = "/v1/appStoreVersions/next/appStoreVersionLocalizations?limit=200";
    const PLACEMENTS: &str = "/v1/appStoreVersionLocalizations/loc-ja/placements\
        ?filter[placementType]=APP_SCREENSHOT&filter[placementGroup]=IPHONE_PROFILE\
        &sort=placementGroupPosition&include=image&limit=200";

    fn screenshots() -> (tempfile::TempDir, AppStore) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("menu.png"), b"menu").unwrap();
        std::fs::write(dir.path().join("detail.png"), b"detail!").unwrap();
        let mut config = AppStore::default();
        config
            .asset_library(|lib| {
                lib.image("menu", |i| {
                    i.file("menu.png");
                });
                lib.image("detail", |i| {
                    i.file("detail.png");
                });
            })
            .version(|v| {
                v.locale("ja", |l| {
                    l.placements("APP_SCREENSHOT", "IPHONE_PROFILE", ["menu", "detail"]);
                });
            });
        (dir, config)
    }

    fn asset_responses() -> Vec<(&'static str, Value)> {
        vec![
            (
                VERSIONS,
                json!({ "data": [
                    { "id": "next", "attributes": { "appVersionState": "PREPARE_FOR_SUBMISSION", "versionString": "1.1" } },
                ]}),
            ),
            (
                "/v1/apps/1/assetLibrary",
                json!({ "data": { "id": "lib" } }),
            ),
            (
                LOCALIZATIONS,
                json!({ "data": [{ "id": "loc-ja", "attributes": { "locale": "ja" } }] }),
            ),
        ]
    }

    #[test]
    fn new_images_are_uploaded_once_then_placed_in_order() {
        let (dir, config) = screenshots();
        let mut responses = asset_responses();
        // `menu` is already in the library; `detail` is not.
        responses.push((
            "/v1/appAssetLibraries/lib/images?filter[referenceName]=menu&limit=200",
            json!({ "data": [{ "id": "img-menu", "attributes": {
                "referenceName": "menu", "fileName": "menu.png", "fileSize": 4,
                "state": "PREPARE_FOR_SUBMISSION",
            }}]}),
        ));
        // The group currently shows one stale placement.
        responses.push((
            PLACEMENTS,
            json!({ "data": [{ "id": "old-placement", "relationships": {
                "image": { "data": { "type": "appAssetLibraryImages", "id": "img-old" } },
            }}]}),
        ));
        responses.push((
            "/v1/appAssetLibraryImages/new1",
            json!({ "data": { "id": "new1", "attributes": { "state": "PREPARE_FOR_SUBMISSION" } } }),
        ));
        let api = fake(&responses);
        let done = push(&api, "com.example.app", &config, dir.path(), &NO_CREATE).unwrap();

        assert_eq!(*api.uploads.borrow(), vec!["detail.png (7 bytes)"]);
        assert_eq!(
            sent(&api),
            [
                ("POST", "/v1/appAssetLibraryImages"),
                ("PATCH", "/v1/appAssetLibraryImages/new1"),
                ("DELETE", "/v1/appAssetLibraryPlacements/old-placement"),
                ("POST", "/v1/appAssetLibraryPlacements"),
                ("POST", "/v1/appAssetLibraryPlacements"),
                ("POST", "/v1/appAssetLibraryPlacementOrderingRequests"),
            ]
            .map(|(method, path)| (method.to_string(), path.to_string()))
        );
        let calls = api.sent.borrow();
        assert_eq!(
            calls[0].2["data"]["attributes"],
            json!({
                "fileName": "detail.png",
                "fileSize": 7,
                "category": "APP_SCREENSHOTS_AND_PREVIEWS",
                "referenceName": "detail",
            })
        );
        assert_eq!(
            calls[1].2["data"]["attributes"],
            json!({ "uploaded": true })
        );
        // First placement shows the reused image, second the new one.
        assert_eq!(
            calls[3].2["data"]["relationships"]["image"]["data"]["id"],
            "img-menu"
        );
        assert_eq!(
            calls[4].2["data"]["relationships"]["image"]["data"]["id"],
            "new1"
        );
        assert_eq!(
            calls[5].2["data"]["relationships"]["orderedPlacements"]["data"],
            json!([
                { "type": "appAssetLibraryPlacements", "id": "new4" },
                { "type": "appAssetLibraryPlacements", "id": "new5" },
            ])
        );
        assert!(done.contains(&"appAssetLibraryImages menu: unchanged".to_string()));
    }

    #[test]
    fn an_image_that_fails_processing_is_not_placed() {
        let (dir, config) = screenshots();
        let mut responses = asset_responses();
        responses.push((
            "/v1/appAssetLibraryImages/new1",
            json!({ "data": { "id": "new1", "attributes": {
                "state": "FAILED",
                "stateDetails": { "errors": [{ "code": "IMAGE_INCORRECT_DIMENSIONS" }] },
            }}}),
        ));
        let api = fake(&responses);
        let err = push(&api, "com.example.app", &config, dir.path(), &NO_CREATE)
            .unwrap_err()
            .to_string();
        assert!(err.contains("rejected image `menu`"), "{err}");
        assert!(err.contains("IMAGE_INCORRECT_DIMENSIONS"), "{err}");
        assert!(
            !sent(&api)
                .iter()
                .any(|(_, path)| path.contains("Placements")),
            "{:?}",
            sent(&api)
        );
    }

    #[test]
    fn an_unchanged_group_writes_nothing() {
        let (dir, config) = screenshots();
        let mut responses = asset_responses();
        for (path, id, file, size) in [
            (
                "/v1/appAssetLibraries/lib/images?filter[referenceName]=menu&limit=200",
                "img-menu",
                "menu.png",
                4,
            ),
            (
                "/v1/appAssetLibraries/lib/images?filter[referenceName]=detail&limit=200",
                "img-detail",
                "detail.png",
                7,
            ),
        ] {
            responses.push((
                path,
                json!({ "data": [{ "id": id, "attributes": {
                    "referenceName": id.trim_start_matches("img-"),
                    "fileName": file, "fileSize": size, "state": "APPROVED",
                }}]}),
            ));
        }
        responses.push((
            PLACEMENTS,
            json!({ "data": [
                { "id": "p1", "relationships": { "image": { "data": { "id": "img-menu" } } } },
                { "id": "p2", "relationships": { "image": { "data": { "id": "img-detail" } } } },
            ]}),
        ));
        let api = fake(&responses);
        push(&api, "com.example.app", &config, dir.path(), &NO_CREATE).unwrap();
        assert!(sent(&api).is_empty(), "{:?}", sent(&api));
        assert!(api.uploads.borrow().is_empty());
    }

    #[test]
    fn asset_mistakes_are_caught_before_anything_is_sent() {
        let (dir, mut config) = screenshots();
        config.version(|v| {
            v.locale("en-US", |l| {
                l.placements("APP_SCREENSHOT", "IPHONE_PROFILE", ["nope"]);
            });
        });
        let err = validate(&config, dir.path()).unwrap_err().to_string();
        assert!(err.contains("places image `nope`"), "{err}");

        let mut config = AppStore::default();
        config.asset_library(|lib| {
            lib.image("menu", |i| {
                i.file("absent.png");
            });
        });
        let err = validate(&config, dir.path()).unwrap_err().to_string();
        assert!(err.contains("`absent.png` does not exist"), "{err}");

        let mut config = AppStore::default();
        config.asset_library(|lib| {
            lib.image("my menu", |i| {
                i.file("menu.png");
            });
        });
        let err = validate(&config, dir.path()).unwrap_err().to_string();
        assert!(err.contains("reference names may only use"), "{err}");
    }
}
