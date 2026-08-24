//! Shared, test-only model for Host conformance scenarios.
//!
//! JSON under `tests/host-conformance` is the language-neutral source of
//! truth. This crate is only the Rust decoder used by Desktop and Web; Kotlin
//! and Swift decode the same versioned schema in their native test targets.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Current manifest and scenario schema version.
pub const SCHEMA_VERSION: u32 = 1;

/// One Host identifier used by the manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Host {
    /// Shared native Desktop renderer.
    Desktop,
    /// Browser DOM renderer.
    Web,
    /// Android View renderer.
    Android,
    /// iOS UIKit renderer.
    Ios,
}

impl Host {
    /// Stable spelling stored in `required_hosts`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Web => "web",
            Self::Android => "android",
            Self::Ios => "ios",
        }
    }
}

/// Top-level list of shared cases and their required Host coverage.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Manifest format version.
    pub schema: u32,
    /// Pinned WPT commit used by every adapted WPT case.
    pub wpt_revision: String,
    /// Cases in deterministic execution order.
    pub cases: Vec<ManifestCase>,
}

/// One entry in [`Manifest`].
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ManifestCase {
    /// Globally unique case identifier.
    pub id: String,
    /// Capability or CSS feature exercised by the case.
    pub feature: String,
    /// Path relative to the Host conformance root.
    pub fixture: PathBuf,
    /// Hosts which must execute this case.
    pub required_hosts: Vec<String>,
    /// Required checkpoint kinds.
    pub checkpoints: Vec<String>,
}

impl ManifestCase {
    /// Whether this case is required for `host`.
    pub fn requires(&self, host: Host) -> bool {
        self.required_hosts
            .iter()
            .any(|value| value == host.as_str())
    }
}

/// One language-neutral Host scenario.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    /// Scenario format version.
    pub schema: u32,
    /// Identifier matching its manifest entry.
    pub id: String,
    /// WPT provenance, present only on WPT adaptations.
    #[serde(default)]
    pub upstream: Option<Upstream>,
    /// Commands sent to the Host under test.
    pub test: ScenarioSide,
    /// Independent reference commands for reftests.
    #[serde(default)]
    pub reference: Option<ScenarioSide>,
}

/// Provenance for a WPT-derived scenario.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Upstream {
    /// Source repository.
    pub repository: String,
    /// Exact source commit.
    pub revision: String,
    /// Source test path.
    pub path: String,
    /// Source reference path, when WPT has one.
    pub reference_path: Option<String>,
    /// Upstream license identifier.
    pub license: String,
    /// Semantic assertion retained by the adaptation.
    pub assertion: String,
    /// Explicit record of adaptation and omitted document behavior.
    pub adaptation: String,
}

/// Ordered commands for one side of a scenario.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScenarioSide {
    /// Commands executed in order.
    pub commands: Vec<Command>,
}

/// Host-boundary command vocabulary shared by every runner.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Creates a Host surface with logical viewport metrics.
    AttachSurface {
        /// Logical width.
        width: f32,
        /// Logical height.
        height: f32,
        /// Physical pixels per logical pixel.
        scale: f32,
    },
    /// Presents one semantic box through the production Host path.
    PresentBox {
        /// Frame target revision.
        revision: u64,
        /// X, y, width, height in logical pixels.
        rect: [f32; 4],
        /// Background color.
        background: ColorFixture,
        /// Optional border semantics.
        #[serde(default)]
        border: Option<BorderFixture>,
    },
    /// Captures one named presentation checkpoint.
    Checkpoint {
        /// Checkpoint contract name.
        name: String,
        /// Optional logical-pixel samples for visual tests without a WPT
        /// reference document.
        #[serde(default)]
        samples: Vec<PixelSampleFixture>,
    },
    /// Sends a text measurement request to the production Host measurer.
    MeasureText {
        /// Measurement correlation key.
        key: u64,
        /// Text content.
        text: String,
        /// Font size in logical pixels.
        font_size: f32,
        /// Line height in logical pixels.
        line_height: f32,
        /// Definite available width.
        available_width: f32,
    },
    /// Checks one previously produced text measurement.
    CheckpointMeasurement {
        /// Measurement correlation key.
        key: u64,
        /// Inclusive minimum width.
        min_width: f32,
        /// Inclusive maximum width.
        max_width: f32,
        /// Inclusive minimum height.
        min_height: f32,
        /// Inclusive maximum height.
        max_height: f32,
        /// Whether reusable prepared content is required.
        prepared_content: bool,
    },
    /// Emits one normalized pointer event into the mock runtime sink.
    EmitPointer {
        /// Pointer event phase.
        event: PointerEventFixture,
        /// Stable pointer identifier.
        pointer_id: u64,
        /// Host monotonic timestamp.
        timestamp_ms: f64,
        /// Logical x coordinate.
        x: f32,
        /// Logical y coordinate.
        y: f32,
        /// Active button bitset.
        buttons: u32,
        /// Changed button, or the Host sentinel.
        changed_button: i16,
    },
    /// Checks the last normalized pointer event.
    CheckpointInput {
        /// Expected pointer event phase.
        event: PointerEventFixture,
        /// Expected pointer identifier.
        pointer_id: u64,
        /// Expected logical x coordinate.
        x: f32,
        /// Expected logical y coordinate.
        y: f32,
    },
}

/// Pointer phase used by input fixtures.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PointerEventFixture {
    /// Pointer pressed.
    Down,
    /// Pointer moved.
    Move,
    /// Pointer released.
    Up,
    /// Pointer sequence cancelled.
    Cancel,
}

/// Color syntax accepted by Host scenarios.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ColorFixture {
    /// CSS named color.
    Named {
        /// Color name.
        value: String,
    },
    /// Explicit sRGB color.
    Srgba {
        /// Red channel.
        red: u8,
        /// Green channel.
        green: u8,
        /// Blue channel.
        blue: u8,
        /// Alpha channel in `[0, 1]`.
        alpha: f32,
    },
}

/// One logical-pixel color assertion captured at a paint checkpoint.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PixelSampleFixture {
    /// Logical x/y coordinate within the attached surface.
    pub point: [f32; 2],
    /// Expected unpremultiplied sRGB color.
    pub color: ColorFixture,
    /// Maximum per-channel difference accepted by native rasterizers.
    #[serde(default)]
    pub tolerance: u8,
}

/// Physical border semantics in top, right, bottom, left order.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BorderFixture {
    /// Widths in logical pixels.
    pub widths: [f32; 4],
    /// Colors.
    pub colors: [ColorFixture; 4],
    /// Line styles.
    pub styles: [BorderStyleFixture; 4],
    /// Circular radii in top-left, top-right, bottom-right, bottom-left order.
    pub radii: [f32; 4],
}

/// Complete CSS border line-style vocabulary.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BorderStyleFixture {
    /// No border.
    None,
    /// Hidden conflict-resolution border.
    Hidden,
    /// Solid line.
    Solid,
    /// Dashed line.
    Dashed,
    /// Dotted line.
    Dotted,
    /// Double line.
    Double,
    /// Grooved line.
    Groove,
    /// Ridged line.
    Ridge,
    /// Inset line.
    Inset,
    /// Outset line.
    Outset,
}

/// A loaded and cross-validated manifest case.
#[derive(Clone, Debug)]
pub struct LoadedCase {
    /// Manifest metadata.
    pub manifest: ManifestCase,
    /// Decoded scenario.
    pub scenario: Scenario,
}

/// Fixture loading or consistency failure.
#[derive(Debug)]
pub struct FixtureError(String);

impl fmt::Display for FixtureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for FixtureError {}

/// Loads every declared case, enforcing manifest/scenario identity and WPT
/// provenance before a platform runner receives any commands.
pub fn load_all(root: &Path) -> Result<(Manifest, Vec<LoadedCase>), FixtureError> {
    let manifest_path = root.join("manifest.json");
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|error| FixtureError(format!("read {}: {error}", manifest_path.display())))?;
    let manifest: Manifest = serde_json::from_str(&manifest_text)
        .map_err(|error| FixtureError(format!("decode {}: {error}", manifest_path.display())))?;
    validate_manifest(&manifest)?;

    let mut loaded = Vec::new();
    for entry in &manifest.cases {
        let path = root.join(&entry.fixture);
        let text = std::fs::read_to_string(&path)
            .map_err(|error| FixtureError(format!("read {}: {error}", path.display())))?;
        let scenario: Scenario = serde_json::from_str(&text)
            .map_err(|error| FixtureError(format!("decode {}: {error}", path.display())))?;
        validate_scenario(&manifest, entry, &scenario)?;
        loaded.push(LoadedCase {
            manifest: entry.clone(),
            scenario,
        });
    }
    Ok((manifest, loaded))
}

/// Loads all cases required by `host` after validating the complete suite.
///
/// Validating first ensures a malformed fixture cannot remain hidden merely
/// because its first Host runner has not been made required yet.
pub fn load_required(root: &Path, host: Host) -> Result<(Manifest, Vec<LoadedCase>), FixtureError> {
    let (manifest, mut loaded) = load_all(root)?;
    loaded.retain(|case| case.manifest.requires(host));
    Ok((manifest, loaded))
}

fn validate_manifest(manifest: &Manifest) -> Result<(), FixtureError> {
    if manifest.schema != SCHEMA_VERSION {
        return Err(FixtureError(format!(
            "manifest schema {} is not supported",
            manifest.schema
        )));
    }
    if manifest.wpt_revision.trim().is_empty() {
        return Err(FixtureError("manifest WPT revision is empty".into()));
    }
    let mut ids = BTreeSet::new();
    let mut fixtures = BTreeSet::new();
    for case in &manifest.cases {
        if !ids.insert(&case.id) {
            return Err(FixtureError(format!("duplicate case id {}", case.id)));
        }
        if !fixtures.insert(&case.fixture) {
            return Err(FixtureError(format!(
                "duplicate fixture {}",
                case.fixture.display()
            )));
        }
        if case.fixture.is_absolute()
            || case
                .fixture
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(FixtureError(format!(
                "fixture path escapes root: {}",
                case.fixture.display()
            )));
        }
        if case.feature.trim().is_empty()
            || case.required_hosts.is_empty()
            || case.checkpoints.is_empty()
        {
            return Err(FixtureError(format!(
                "case {} has incomplete metadata",
                case.id
            )));
        }
        let mut hosts = BTreeSet::new();
        for host in &case.required_hosts {
            if !matches!(host.as_str(), "desktop" | "web" | "android" | "ios") {
                return Err(FixtureError(format!(
                    "case {} names unknown Host {host}",
                    case.id
                )));
            }
            if !hosts.insert(host) {
                return Err(FixtureError(format!(
                    "case {} repeats required Host {host}",
                    case.id
                )));
            }
        }
        let mut checkpoints = BTreeSet::new();
        for checkpoint in &case.checkpoints {
            if !matches!(
                checkpoint.as_str(),
                "rust-layout-protocol"
                    | "semantic-projection"
                    | "pixel"
                    | "pixel-samples"
                    | "measurement"
                    | "input"
            ) {
                return Err(FixtureError(format!(
                    "case {} names unknown checkpoint {checkpoint}",
                    case.id
                )));
            }
            if !checkpoints.insert(checkpoint) {
                return Err(FixtureError(format!(
                    "case {} repeats checkpoint {checkpoint}",
                    case.id
                )));
            }
        }
    }
    Ok(())
}

fn validate_scenario(
    manifest: &Manifest,
    entry: &ManifestCase,
    scenario: &Scenario,
) -> Result<(), FixtureError> {
    if scenario.schema != SCHEMA_VERSION {
        return Err(FixtureError(format!(
            "scenario {} schema {} is not supported",
            scenario.id, scenario.schema
        )));
    }
    if scenario.id != entry.id {
        return Err(FixtureError(format!(
            "fixture {} declares id {}, expected {}",
            entry.fixture.display(),
            scenario.id,
            entry.id
        )));
    }
    if scenario.test.commands.is_empty() {
        return Err(FixtureError(format!(
            "scenario {} has no test commands",
            scenario.id
        )));
    }
    validate_side(&scenario.id, "test", &scenario.test)?;
    if entry
        .checkpoints
        .iter()
        .any(|checkpoint| checkpoint == "pixel-samples")
        && !scenario.test.commands.iter().any(|command| {
            matches!(
                command,
                Command::Checkpoint { samples, .. } if !samples.is_empty()
            )
        })
    {
        return Err(FixtureError(format!(
            "scenario {} declares pixel-samples without sample assertions",
            scenario.id
        )));
    }
    if let Some(reference) = &scenario.reference {
        validate_side(&scenario.id, "reference", reference)?;
    }
    if scenario.id.starts_with("wpt.") {
        let upstream = scenario.upstream.as_ref().ok_or_else(|| {
            FixtureError(format!("WPT scenario {} has no provenance", scenario.id))
        })?;
        if upstream.repository != "https://github.com/web-platform-tests/wpt"
            || upstream.revision != manifest.wpt_revision
            || upstream.license != "BSD-3-Clause"
            || upstream.path.trim().is_empty()
            || upstream.assertion.trim().is_empty()
            || upstream.adaptation.trim().is_empty()
        {
            return Err(FixtureError(format!(
                "WPT scenario {} has invalid provenance",
                scenario.id
            )));
        }
    } else if scenario.upstream.is_some() {
        return Err(FixtureError(format!(
            "core scenario {} unexpectedly has WPT provenance",
            scenario.id
        )));
    }
    Ok(())
}

fn validate_side(id: &str, label: &str, side: &ScenarioSide) -> Result<(), FixtureError> {
    if side.commands.is_empty() {
        return Err(FixtureError(format!(
            "scenario {id} has no {label} commands"
        )));
    }
    for command in &side.commands {
        match command {
            Command::AttachSurface {
                width,
                height,
                scale,
            } if finite_positive(*width) && finite_positive(*height) && finite_positive(*scale) => {
            }
            Command::PresentBox {
                revision,
                rect,
                background,
                border,
            } if *revision > 0
                && rect.iter().all(|value| value.is_finite())
                && rect[2] >= 0.0
                && rect[3] >= 0.0
                && valid_color(background)
                && border.as_ref().is_none_or(valid_border) => {}
            Command::Checkpoint { name, samples }
                if !name.trim().is_empty()
                    && samples.iter().all(|sample| {
                        sample
                            .point
                            .iter()
                            .all(|value| value.is_finite() && *value >= 0.0)
                            && valid_color(&sample.color)
                    }) => {}
            Command::MeasureText {
                key,
                font_size,
                line_height,
                available_width,
                ..
            } if *key > 0
                && finite_positive(*font_size)
                && finite_positive(*line_height)
                && available_width.is_finite()
                && *available_width >= 0.0 => {}
            Command::CheckpointMeasurement {
                key,
                min_width,
                max_width,
                min_height,
                max_height,
                ..
            } if *key > 0
                && [*min_width, *max_width, *min_height, *max_height]
                    .iter()
                    .all(|value| value.is_finite() && *value >= 0.0)
                && min_width <= max_width
                && min_height <= max_height => {}
            Command::EmitPointer {
                timestamp_ms, x, y, ..
            } if timestamp_ms.is_finite()
                && *timestamp_ms >= 0.0
                && x.is_finite()
                && y.is_finite() => {}
            Command::CheckpointInput { x, y, .. } if x.is_finite() && y.is_finite() => {}
            _ => {
                return Err(FixtureError(format!(
                    "scenario {id} has an invalid {label} command"
                )));
            }
        }
    }
    Ok(())
}

fn finite_positive(value: f32) -> bool {
    value.is_finite() && value > 0.0
}

fn valid_color(color: &ColorFixture) -> bool {
    match color {
        ColorFixture::Named { value } => !value.trim().is_empty(),
        ColorFixture::Srgba { alpha, .. } => alpha.is_finite() && (0.0..=1.0).contains(alpha),
    }
}

fn valid_border(border: &BorderFixture) -> bool {
    border
        .widths
        .iter()
        .chain(border.radii.iter())
        .all(|value| value.is_finite() && *value >= 0.0)
        && border.colors.iter().all(valid_color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_suite_is_well_formed() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/host-conformance");
        let (manifest, all) = load_all(&root).unwrap();
        let (_, desktop) = load_required(&root, Host::Desktop).unwrap();
        assert_eq!(all.len(), manifest.cases.len());
        assert_eq!(
            desktop.len(),
            manifest
                .cases
                .iter()
                .filter(|case| case.requires(Host::Desktop))
                .count()
        );
        assert!(desktop.iter().any(|case| case.scenario.reference.is_some()));
        assert!(desktop.iter().any(|case| case.scenario.upstream.is_none()));
    }
}
