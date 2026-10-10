//! The configuration half of `store.rs` — an executable an app keeps
//! next to `whisker.rs` to declare what the stores hold besides the
//! binary:
//!
//! ```ignore
//! fn main() {
//!     whisker::store::run(|store| {
//!         store.playstore(|p| {
//!             p.listing("ja-JP", |l| {
//!                 l.title("GIGA")
//!                     .full_description(include_str!("metadata/ja/description.txt"));
//!             });
//!             p.release(|r| {
//!                 r.release_notes("ja-JP", include_str!("metadata/ja/release_notes.txt"));
//!             });
//!         });
//!         store.appstore(|a| {
//!             a.app_info(|i| {
//!                 i.locale("ja", |l| {
//!                     l.name("GIGA");
//!                 });
//!             });
//!             a.version(|v| {
//!                 v.locale("ja", |l| {
//!                     l.description(include_str!("metadata/ja/description.txt"));
//!                 });
//!             });
//!         });
//!     });
//! }
//! ```
//!
//! Each store is declared on its own, mirroring that store's API:
//! every block is one API resource and every setter one of its
//! attributes, under the API's own name in snake_case. The stores
//! differ in what exists, what it is attached to (a Play release, an
//! App Store version, a TestFlight build), when it may change, and
//! even how languages are spelled (`ja-JP` vs `ja`); a shared shape
//! would hide exactly the differences an author has to know, and the
//! stores' API references double as this file's documentation.
//!
//! Only what is set is sent: an unset attribute, or a locale that is
//! not declared, is left as the store has it.
//!
//! It is separate from `whisker.rs` because nothing here reaches the
//! generated native project: a broken release note must not be able
//! to stop `whisker run`.

use serde::{Deserialize, Serialize};

/// Bumped whenever the report's shape changes incompatibly.
pub const SCHEMA_VERSION: u32 = 1;

/// What [`run`] hands back to the CLI.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct StoreReport {
    pub schema_version: u32,
    pub config: StoreConfig,
}

/// One `Option<String>` setter per attribute.
macro_rules! text_setters {
    ($($(#[$doc:meta])* $name:ident),* $(,)?) => {
        $(
            $(#[$doc])*
            pub fn $name(&mut self, value: impl Into<String>) -> &mut Self {
                self.$name = Some(value.into());
                self
            }
        )*
    };
}

/// One `Option<Vec<String>>` setter per ordered file list. Declaring
/// an empty list means "the store should have none".
macro_rules! file_list_setters {
    ($($(#[$doc:meta])* $name:ident),* $(,)?) => {
        $(
            $(#[$doc])*
            pub fn $name<T: Into<String>>(
                &mut self,
                files: impl IntoIterator<Item = T>,
            ) -> &mut Self {
                self.$name = Some(files.into_iter().map(Into::into).collect());
                self
            }
        )*
    };
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct StoreConfig {
    pub playstore: PlayStore,
    pub appstore: AppStore,
}

impl StoreConfig {
    /// What Google Play holds for this app.
    pub fn playstore(&mut self, f: impl FnOnce(&mut PlayStore)) -> &mut Self {
        f(&mut self.playstore);
        self
    }

    /// What App Store Connect holds for this app.
    pub fn appstore(&mut self, f: impl FnOnce(&mut AppStore)) -> &mut Self {
        f(&mut self.appstore);
        self
    }
}

// ---- Google Play -----------------------------------------------------

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PlayStore {
    pub details: PlayDetails,
    pub listings: Vec<PlayListing>,
    pub images: Vec<PlayImages>,
    pub release: PlayRelease,
}

impl PlayStore {
    /// `edits.details` — app-wide contact details. Sent by
    /// `whisker store push playstore`.
    pub fn details(&mut self, f: impl FnOnce(&mut PlayDetails)) -> &mut Self {
        f(&mut self.details);
        self
    }

    /// `edits.listings` — the store page text for one language
    /// (a Play code such as `en-US` or `ja-JP`). Sent by
    /// `whisker store push playstore`.
    pub fn listing(
        &mut self,
        language: impl Into<String>,
        f: impl FnOnce(&mut PlayListing),
    ) -> &mut Self {
        let mut listing = PlayListing {
            language: language.into(),
            ..PlayListing::default()
        };
        f(&mut listing);
        self.listings.push(listing);
        self
    }

    /// `edits.images` — the store page images for one language.
    /// Each image type that is set replaces what Play has for it;
    /// types left unset are untouched. Paths are relative to the
    /// app's `Cargo.toml`. Sent by `whisker store push playstore`.
    pub fn images(
        &mut self,
        language: impl Into<String>,
        f: impl FnOnce(&mut PlayImages),
    ) -> &mut Self {
        let mut images = PlayImages {
            language: language.into(),
            ..PlayImages::default()
        };
        f(&mut images);
        self.images.push(images);
        self
    }

    /// `edits.tracks` `releases[]` — what accompanies a release on a
    /// track. Sent by `whisker submit android --track`.
    pub fn release(&mut self, f: impl FnOnce(&mut PlayRelease)) -> &mut Self {
        f(&mut self.release);
        self
    }
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PlayDetails {
    pub default_language: Option<String>,
    pub contact_email: Option<String>,
    pub contact_phone: Option<String>,
    pub contact_website: Option<String>,
}

impl PlayDetails {
    text_setters!(
        default_language,
        contact_email,
        contact_phone,
        contact_website
    );
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PlayListing {
    pub language: String,
    pub title: Option<String>,
    pub short_description: Option<String>,
    pub full_description: Option<String>,
    pub video: Option<String>,
}

impl PlayListing {
    text_setters!(
        /// The app's name on the store page (30 characters).
        title,
        /// 80 characters.
        short_description,
        /// 4000 characters.
        full_description,
        /// URL of a promotional YouTube video.
        video,
    );
}

/// Field names are Play's `AppImageType` values in snake_case.
#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PlayImages {
    pub language: String,
    pub icon: Option<String>,
    pub feature_graphic: Option<String>,
    pub tv_banner: Option<String>,
    pub phone_screenshots: Option<Vec<String>>,
    pub seven_inch_screenshots: Option<Vec<String>>,
    pub ten_inch_screenshots: Option<Vec<String>>,
    pub tv_screenshots: Option<Vec<String>>,
    pub wear_screenshots: Option<Vec<String>>,
}

impl PlayImages {
    text_setters!(icon, feature_graphic, tv_banner);

    file_list_setters!(
        /// In display order.
        phone_screenshots,
        seven_inch_screenshots,
        ten_inch_screenshots,
        tv_screenshots,
        wear_screenshots,
    );
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PlayRelease {
    pub release_notes: Vec<LocalizedText>,
}

impl PlayRelease {
    /// The release's "What's new" text in one language (500
    /// characters).
    pub fn release_notes(
        &mut self,
        language: impl Into<String>,
        text: impl Into<String>,
    ) -> &mut Self {
        self.release_notes.push(LocalizedText {
            language: language.into(),
            text: text.into(),
        });
        self
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct LocalizedText {
    pub language: String,
    pub text: String,
}

// ---- App Store Connect -----------------------------------------------

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppStore {
    pub app: App,
    pub app_info: AppInfo,
    pub asset_library: AssetLibrary,
    pub version: AppStoreVersion,
    pub review_detail: ReviewDetail,
    pub beta_app: BetaApp,
    pub beta_app_review_detail: ReviewDetail,
    pub beta_build: BetaBuild,
}

impl AppStore {
    /// `apps` — attributes of the app record itself. Sent by
    /// `whisker store push appstore`.
    pub fn app(&mut self, f: impl FnOnce(&mut App)) -> &mut Self {
        f(&mut self.app);
        self
    }

    /// `appInfos` — categories and the localized name. Attached to
    /// the app, but App Store Connect only lets it change while a
    /// version is being prepared, and it goes live with that version.
    /// Sent by `whisker store push appstore`.
    pub fn app_info(&mut self, f: impl FnOnce(&mut AppInfo)) -> &mut Self {
        f(&mut self.app_info);
        self
    }

    /// `appAssetLibraries` — the app's reusable images. An image is
    /// uploaded here once and then shown wherever a placement refers
    /// to it. Sent by `whisker store push appstore`.
    pub fn asset_library(&mut self, f: impl FnOnce(&mut AssetLibrary)) -> &mut Self {
        f(&mut self.asset_library);
        self
    }

    /// `appStoreVersions` — the version being prepared for
    /// submission; goes live when that version is released. Sent by
    /// `whisker store push appstore`.
    pub fn version(&mut self, f: impl FnOnce(&mut AppStoreVersion)) -> &mut Self {
        f(&mut self.version);
        self
    }

    /// `appStoreReviewDetails` — what App Review needs for the
    /// version being prepared. Sent by `whisker store push appstore`.
    pub fn review_detail(&mut self, f: impl FnOnce(&mut ReviewDetail)) -> &mut Self {
        f(&mut self.review_detail);
        self
    }

    /// `betaAppLocalizations` — TestFlight's app-wide test
    /// information. Sent by `whisker store push appstore`.
    pub fn beta_app(&mut self, f: impl FnOnce(&mut BetaApp)) -> &mut Self {
        f(&mut self.beta_app);
        self
    }

    /// `betaAppReviewDetails` — what Beta App Review needs. Sent by
    /// `whisker store push appstore`.
    pub fn beta_app_review_detail(&mut self, f: impl FnOnce(&mut ReviewDetail)) -> &mut Self {
        f(&mut self.beta_app_review_detail);
        self
    }

    /// `betaBuildLocalizations` — what accompanies one build in
    /// TestFlight. Sent by `whisker submit ios` once the build is
    /// processed.
    pub fn beta_build(&mut self, f: impl FnOnce(&mut BetaBuild)) -> &mut Self {
        f(&mut self.beta_build);
        self
    }
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct App {
    pub content_rights_declaration: Option<String>,
    pub primary_locale: Option<String>,
}

impl App {
    text_setters!(
        /// `DOES_NOT_USE_THIRD_PARTY_CONTENT` or
        /// `USES_THIRD_PARTY_CONTENT`.
        content_rights_declaration,
        primary_locale,
    );
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppInfo {
    pub primary_category: Option<String>,
    pub primary_subcategory_one: Option<String>,
    pub primary_subcategory_two: Option<String>,
    pub secondary_category: Option<String>,
    pub secondary_subcategory_one: Option<String>,
    pub secondary_subcategory_two: Option<String>,
    pub localizations: Vec<AppInfoLocalization>,
}

impl AppInfo {
    text_setters!(
        /// An `appCategories` id such as `BOOKS` or `GAMES`.
        primary_category,
        primary_subcategory_one,
        primary_subcategory_two,
        secondary_category,
        secondary_subcategory_one,
        secondary_subcategory_two,
    );

    /// `appInfoLocalizations` for one locale (an App Store code such
    /// as `en-US` or `ja`).
    pub fn locale(
        &mut self,
        locale: impl Into<String>,
        f: impl FnOnce(&mut AppInfoLocalization),
    ) -> &mut Self {
        let mut localization = AppInfoLocalization {
            locale: locale.into(),
            ..AppInfoLocalization::default()
        };
        f(&mut localization);
        self.localizations.push(localization);
        self
    }
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppInfoLocalization {
    pub locale: String,
    pub name: Option<String>,
    pub subtitle: Option<String>,
    pub privacy_policy_url: Option<String>,
    pub privacy_choices_url: Option<String>,
    pub privacy_policy_text: Option<String>,
}

impl AppInfoLocalization {
    text_setters!(
        /// 2–30 characters.
        name,
        /// 30 characters.
        subtitle,
        privacy_policy_url,
        privacy_choices_url,
        /// tvOS only.
        privacy_policy_text,
    );
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AssetLibrary {
    pub images: Vec<AssetLibraryImage>,
}

impl AssetLibrary {
    /// `appAssetLibraryImages` — one image, identified by its
    /// `referenceName`, which is also how placements refer to it.
    pub fn image(
        &mut self,
        reference_name: impl Into<String>,
        f: impl FnOnce(&mut AssetLibraryImage),
    ) -> &mut Self {
        let mut image = AssetLibraryImage {
            reference_name: reference_name.into(),
            ..AssetLibraryImage::default()
        };
        f(&mut image);
        self.images.push(image);
        self
    }
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AssetLibraryImage {
    pub reference_name: String,
    pub file: Option<String>,
    pub category: Option<String>,
}

impl AssetLibraryImage {
    text_setters!(
        /// Path relative to the app's `Cargo.toml`. Its dimensions
        /// must match one of App Store Connect's image specifications
        /// exactly.
        file,
        /// `APP_SCREENSHOTS_AND_PREVIEWS` (the default) or
        /// `CREATIVE_ASSETS`.
        category,
    );
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppStoreVersion {
    pub copyright: Option<String>,
    pub release_type: Option<String>,
    pub earliest_release_date: Option<String>,
    pub localizations: Vec<AppStoreVersionLocalization>,
}

impl AppStoreVersion {
    text_setters!(
        copyright,
        /// `MANUAL`, `AFTER_APPROVAL`, or `SCHEDULED`.
        release_type,
        /// RFC 3339 date-time, for `SCHEDULED`.
        earliest_release_date,
    );

    /// `appStoreVersionLocalizations` for one locale (an App Store
    /// code such as `en-US` or `ja`).
    pub fn locale(
        &mut self,
        locale: impl Into<String>,
        f: impl FnOnce(&mut AppStoreVersionLocalization),
    ) -> &mut Self {
        let mut localization = AppStoreVersionLocalization {
            locale: locale.into(),
            ..AppStoreVersionLocalization::default()
        };
        f(&mut localization);
        self.localizations.push(localization);
        self
    }
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppStoreVersionLocalization {
    pub locale: String,
    pub description: Option<String>,
    pub keywords: Option<String>,
    pub whats_new: Option<String>,
    pub promotional_text: Option<String>,
    pub marketing_url: Option<String>,
    pub support_url: Option<String>,
    pub placements: Vec<Placements>,
}

/// The ordered `appAssetLibraryPlacements` of one placement type and
/// group on a localization.
#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Placements {
    pub placement_type: String,
    pub placement_group: String,
    pub images: Vec<String>,
}

impl AppStoreVersionLocalization {
    /// What this localization shows for one placement type (such as
    /// `APP_SCREENSHOT`) and group (a device family such as
    /// `IPHONE_DYNAMIC_ISLAND_LARGE_PROFILE`, as listed by App Store
    /// Connect's reference data): the asset library images with
    /// these reference names, in display order. Replaces what the
    /// group has; groups not declared are untouched.
    pub fn placements<T: Into<String>>(
        &mut self,
        placement_type: impl Into<String>,
        placement_group: impl Into<String>,
        images: impl IntoIterator<Item = T>,
    ) -> &mut Self {
        self.placements.push(Placements {
            placement_type: placement_type.into(),
            placement_group: placement_group.into(),
            images: images.into_iter().map(Into::into).collect(),
        });
        self
    }

    text_setters!(
        /// 4000 characters.
        description,
        /// Comma-separated, 100 characters.
        keywords,
        /// "What's New in This Version" (4000 characters). App Store
        /// Connect rejects it on an app's first version.
        whats_new,
        /// 170 characters.
        promotional_text,
        marketing_url,
        support_url,
    );
}

/// Shared by `appStoreReviewDetails` and `betaAppReviewDetails`,
/// whose attributes are the same.
///
/// The demo account's name and password are deliberately absent:
/// `store.rs` is plaintext in the repository.
#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ReviewDetail {
    pub contact_first_name: Option<String>,
    pub contact_last_name: Option<String>,
    pub contact_phone: Option<String>,
    pub contact_email: Option<String>,
    pub demo_account_required: Option<bool>,
    pub notes: Option<String>,
}

impl ReviewDetail {
    text_setters!(
        contact_first_name,
        contact_last_name,
        /// International format with a leading `+`.
        contact_phone,
        contact_email,
        /// 4000 characters.
        notes,
    );

    pub fn demo_account_required(&mut self, required: bool) -> &mut Self {
        self.demo_account_required = Some(required);
        self
    }
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BetaApp {
    pub localizations: Vec<BetaAppLocalization>,
}

impl BetaApp {
    /// `betaAppLocalizations` for one locale.
    pub fn locale(
        &mut self,
        locale: impl Into<String>,
        f: impl FnOnce(&mut BetaAppLocalization),
    ) -> &mut Self {
        let mut localization = BetaAppLocalization {
            locale: locale.into(),
            ..BetaAppLocalization::default()
        };
        f(&mut localization);
        self.localizations.push(localization);
        self
    }
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BetaAppLocalization {
    pub locale: String,
    pub description: Option<String>,
    pub feedback_email: Option<String>,
    pub marketing_url: Option<String>,
    pub privacy_policy_url: Option<String>,
    pub tv_os_privacy_policy: Option<String>,
}

impl BetaAppLocalization {
    text_setters!(
        description,
        feedback_email,
        marketing_url,
        privacy_policy_url,
        tv_os_privacy_policy,
    );
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BetaBuild {
    pub localizations: Vec<BetaBuildLocalization>,
}

impl BetaBuild {
    /// `betaBuildLocalizations` for one locale.
    pub fn locale(
        &mut self,
        locale: impl Into<String>,
        f: impl FnOnce(&mut BetaBuildLocalization),
    ) -> &mut Self {
        let mut localization = BetaBuildLocalization {
            locale: locale.into(),
            ..BetaBuildLocalization::default()
        };
        f(&mut localization);
        self.localizations.push(localization);
        self
    }
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BetaBuildLocalization {
    pub locale: String,
    pub whats_new: Option<String>,
}

impl BetaBuildLocalization {
    text_setters!(
        /// TestFlight's "What to Test".
        whats_new,
    );
}

/// Entry point for `store.rs`: build the config and report it to the
/// CLI that launched this executable. Run by hand (no
/// `--report-path`), it prints the report instead.
pub fn run(configure: impl FnOnce(&mut StoreConfig)) {
    let mut config = StoreConfig::default();
    configure(&mut config);
    let report = serde_json::to_string_pretty(&StoreReport {
        schema_version: SCHEMA_VERSION,
        config,
    })
    .expect("store config serializes");

    let mut args = std::env::args().skip(1);
    let mut report_path = None;
    while let Some(arg) = args.next() {
        if arg == "--report-path" {
            report_path = args.next();
        }
    }
    match report_path {
        Some(path) => {
            if let Err(e) = std::fs::write(&path, report) {
                eprintln!("store.rs: cannot write report to {path}: {e}");
                std::process::exit(1);
            }
        }
        None => println!("{report}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> StoreConfig {
        let mut config = StoreConfig::default();
        config
            .playstore(|p| {
                p.details(|d| {
                    d.contact_email("support@example.com");
                });
                p.listing("ja-JP", |l| {
                    l.title("アプリ").short_description("短い説明");
                });
                p.release(|r| {
                    r.release_notes("ja-JP", "修正");
                });
            })
            .appstore(|a| {
                a.app_info(|i| {
                    i.primary_category("BOOKS");
                    i.locale("ja", |l| {
                        l.name("アプリ");
                    });
                });
                a.version(|v| {
                    v.copyright("2026 Example");
                    v.locale("ja", |l| {
                        l.description("説明");
                    });
                });
                a.review_detail(|r| {
                    r.demo_account_required(false);
                });
                a.beta_build(|b| {
                    b.locale("ja", |l| {
                        l.whats_new("修正");
                    });
                });
            });
        config
    }

    #[test]
    fn each_store_keeps_its_own_locales_and_names() {
        let config = config();
        assert_eq!(config.playstore.listings[0].language, "ja-JP");
        assert_eq!(
            config.playstore.listings[0].title.as_deref(),
            Some("アプリ")
        );
        assert_eq!(config.appstore.app_info.localizations[0].locale, "ja");
        assert_eq!(
            config.appstore.app_info.localizations[0].name.as_deref(),
            Some("アプリ")
        );
        assert_eq!(
            config.appstore.review_detail.demo_account_required,
            Some(false)
        );
    }

    #[test]
    fn report_round_trips_and_tolerates_missing_sections() {
        let report = StoreReport {
            schema_version: SCHEMA_VERSION,
            config: config(),
        };
        let json = serde_json::to_string(&report).unwrap();
        assert_eq!(serde_json::from_str::<StoreReport>(&json).unwrap(), report);

        let minimal: StoreReport =
            serde_json::from_str(r#"{"schema_version":1,"config":{}}"#).unwrap();
        assert_eq!(minimal.config, StoreConfig::default());
    }
}
