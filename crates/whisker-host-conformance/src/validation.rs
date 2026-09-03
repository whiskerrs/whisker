use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use super::*;

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
                    | "pixel-relations"
                    | "resource-registration"
                    | "resource-lifecycle"
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
    if entry
        .checkpoints
        .iter()
        .any(|checkpoint| checkpoint == "resource-lifecycle")
        && !scenario
            .test
            .commands
            .iter()
            .any(|command| matches!(command, Command::CheckpointResource { .. }))
    {
        return Err(FixtureError(format!(
            "scenario {} declares resource-lifecycle without a resource checkpoint",
            scenario.id
        )));
    }
    if entry
        .checkpoints
        .iter()
        .any(|checkpoint| checkpoint == "pixel-relations")
        && !scenario.test.commands.iter().any(|command| {
            matches!(
                command,
                Command::Checkpoint { relations, .. } if !relations.is_empty()
            )
        })
    {
        return Err(FixtureError(format!(
            "scenario {} declares pixel-relations without relation assertions",
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
            Command::RegisterRasterResource {
                id,
                width,
                height,
                pixels,
            } if *id > 0
                && *width > 0
                && *height > 0
                && width
                    .checked_mul(*height)
                    .is_some_and(|count| count as usize == pixels.len())
                && pixels.iter().all(valid_color) => {}
            Command::LoadRasterResource {
                id,
                generation,
                source,
            } if *id > 0 && *generation > 0 && valid_resource_source(source) => {}
            Command::ReleaseRasterResource { id, generation } if *id > 0 && *generation > 0 => {}
            Command::CheckpointResource {
                id,
                generation,
                state,
                width,
                height,
            } if *id > 0
                && *generation > 0
                && match state {
                    ResourceStateFixture::Ready => {
                        width.is_some_and(|value| value > 0)
                            && height.is_some_and(|value| value > 0)
                    }
                    ResourceStateFixture::Failed | ResourceStateFixture::Released => {
                        width.is_none() && height.is_none()
                    }
                } => {}
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
            Command::PresentScene { revision, nodes }
                if *revision > 0 && valid_scene_nodes(nodes) => {}
            Command::Checkpoint {
                name,
                samples,
                relations,
            } if !name.trim().is_empty()
                && samples.iter().all(|sample| {
                    sample
                        .point
                        .iter()
                        .all(|value| value.is_finite() && *value >= 0.0)
                        && valid_color(&sample.color)
                })
                && relations.iter().all(|relation| {
                    relation
                        .first
                        .iter()
                        .chain(relation.second.iter())
                        .all(|value| value.is_finite() && *value >= 0.0)
                }) => {}
            Command::MeasureText {
                key,
                font_families,
                font_size,
                font_weight,
                line_height,
                letter_spacing,
                font_features,
                font_variations,
                indent,
                available_width,
                ..
            } if *key > 0
                && !font_families.is_empty()
                && font_families.iter().all(|family| !family.is_empty())
                && finite_positive(*font_size)
                && (1..=1000).contains(font_weight)
                && finite_positive(*line_height)
                && letter_spacing.is_finite()
                && font_features
                    .iter()
                    .all(|feature| valid_font_tag(&feature.tag))
                && font_variations.iter().all(|variation| {
                    valid_font_tag(&variation.tag) && variation.value.is_finite()
                })
                && indent.logical_pixels.is_finite()
                && indent.percentage.is_finite()
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
                pointer_id,
                timestamp_ms,
                x,
                y,
                ..
            } if *pointer_id > 0
                && timestamp_ms.is_finite()
                && *timestamp_ms >= 0.0
                && x.is_finite()
                && y.is_finite() => {}
            Command::CheckpointInput {
                pointer_id, x, y, ..
            } if *pointer_id > 0 && x.is_finite() && y.is_finite() => {}
            _ => {
                return Err(FixtureError(format!(
                    "scenario {id} has an invalid {label} command"
                )));
            }
        }
    }
    Ok(())
}

fn valid_resource_source(source: &ResourceSourceFixture) -> bool {
    match source {
        ResourceSourceFixture::Url { value } => !value.trim().is_empty(),
        ResourceSourceFixture::Bytes { media_type, base64 } => {
            !media_type.trim().is_empty()
                && !base64.trim().is_empty()
                && base64
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
        }
    }
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
        .all(|value| value.is_finite() && *value >= 0.0)
        && border.radii.iter().all(|radius| radius.is_valid())
        && border.colors.iter().all(valid_color)
}

fn valid_background_geometry(geometry: &BackgroundLayerFixture) -> bool {
    geometry
        .position
        .iter()
        .all(LengthPercentageFixture::is_finite)
        && match geometry.size {
            BackgroundSizeFixture::ExplicitPair(size) => {
                size.iter().all(LengthPercentageFixture::is_finite)
            }
            BackgroundSizeFixture::ExplicitAxes { width, height } => {
                (width.is_some() || height.is_some())
                    && width.is_none_or(|value| value.is_finite())
                    && height.is_none_or(|value| value.is_finite())
            }
            BackgroundSizeFixture::Keyword(_) => true,
        }
}

fn valid_linear_gradient(gradient: &LinearGradientFixture) -> bool {
    gradient.angle_degrees.is_finite()
        && gradient.stops.len() >= 2
        && gradient
            .stops
            .iter()
            .all(|stop| valid_color(&stop.color) && stop.position.is_finite())
}

fn valid_radial_gradient(gradient: &RadialGradientFixture) -> bool {
    gradient.center.iter().all(|value| value.is_finite())
        && gradient
            .radii
            .iter()
            .all(|value| value.is_finite() && *value > 0.0)
        && gradient.stops.len() >= 2
        && gradient
            .stops
            .iter()
            .all(|stop| valid_color(&stop.color) && stop.position.is_finite())
}

fn valid_conic_gradient(gradient: &ConicGradientFixture) -> bool {
    gradient.from_degrees.is_finite()
        && gradient.center.iter().all(|value| value.is_finite())
        && gradient.stops.len() >= 2
        && gradient
            .stops
            .iter()
            .all(|stop| valid_color(&stop.color) && stop.position.is_finite())
}

fn valid_background_image(image: &BackgroundImageFixture) -> bool {
    match image {
        BackgroundImageFixture::Resource(resource) => *resource != 0,
        BackgroundImageFixture::LinearGradient(gradient) => valid_linear_gradient(gradient),
        BackgroundImageFixture::RadialGradient(gradient) => valid_radial_gradient(gradient),
        BackgroundImageFixture::ConicGradient(gradient) => valid_conic_gradient(gradient),
    }
}

fn valid_scene_nodes(nodes: &[SceneNodeFixture]) -> bool {
    if nodes.is_empty() {
        return false;
    }
    let ids = nodes
        .iter()
        .map(|node| node.id)
        .collect::<std::collections::BTreeSet<_>>();
    ids.len() == nodes.len()
        && !ids.contains(&0)
        && nodes.iter().all(|node| {
            node.parent
                .is_none_or(|parent| parent != node.id && ids.contains(&parent))
                && node.rect.iter().all(|value| value.is_finite())
                && node.rect[2] >= 0.0
                && node.rect[3] >= 0.0
                && node.content_box.is_none_or(|rect| {
                    rect.into_iter().all(f32::is_finite) && rect[2] >= 0.0 && rect[3] >= 0.0
                })
                && valid_color(&node.background)
                && node.text.as_ref().is_none_or(|text| {
                    text.font_size.is_finite()
                        && text.font_size > 0.0
                        && (1..=1000).contains(&text.font_weight)
                        && !text.font_families.is_empty()
                        && text.font_families.iter().all(|family| !family.is_empty())
                        && text
                            .line_height
                            .is_none_or(|height| height.is_finite() && height > 0.0)
                        && text.letter_spacing.is_finite()
                        && text
                            .font_features
                            .iter()
                            .all(|feature| valid_font_tag(&feature.tag))
                        && text.font_variations.iter().all(|variation| {
                            valid_font_tag(&variation.tag) && variation.value.is_finite()
                        })
                        && valid_color(&text.color)
                        && text
                            .decoration
                            .as_ref()
                            .is_none_or(|decoration| valid_color(&decoration.color))
                        && text.shadow.as_ref().is_none_or(|shadow| {
                            shadow.offset.into_iter().all(f32::is_finite)
                                && shadow.blur_radius.is_finite()
                                && shadow.blur_radius >= 0.0
                                && valid_color(&shadow.color)
                        })
                })
                && node.border.as_ref().is_none_or(valid_border)
                && node.box_shadows.iter().all(|shadow| {
                    shadow.offset.into_iter().all(f32::is_finite)
                        && shadow.blur_radius.is_finite()
                        && shadow.blur_radius >= 0.0
                        && shadow.spread_radius.is_finite()
                        && valid_color(&shadow.color)
                })
                && node
                    .backdrop_blur
                    .is_none_or(|radius| radius.is_finite() && radius >= 0.0)
                && node
                    .clip_path
                    .as_ref()
                    .is_none_or(|clip| match &clip.shape {
                        ClipShapeFixture::Inset { edges, radii } => {
                            edges.iter().all(LengthPercentageFixture::is_finite)
                                && radii.iter().copied().all(CornerRadiusFixture::is_valid)
                        }
                        ClipShapeFixture::Circle { radius, center } => {
                            radius.is_non_negative()
                                && center.iter().all(LengthPercentageFixture::is_finite)
                        }
                        ClipShapeFixture::Ellipse { radii, center } => {
                            radii.iter().all(LengthPercentageFixture::is_non_negative)
                                && center.iter().all(LengthPercentageFixture::is_finite)
                        }
                        ClipShapeFixture::Path { commands, .. } => {
                            !commands.is_empty()
                                && commands.iter().all(PathCommandFixture::is_valid)
                        }
                    })
                && node
                    .transform
                    .is_none_or(|transform| transform.into_iter().all(f32::is_finite))
                && node
                    .opacity
                    .is_none_or(|opacity| opacity.is_finite() && (0.0..=1.0).contains(&opacity))
                && valid_background_geometry(&node.background_layer)
                && node.background_layers.iter().all(|layer| {
                    valid_background_geometry(&layer.geometry)
                        && valid_background_image(&layer.image)
                })
                && node
                    .linear_gradient
                    .as_ref()
                    .is_none_or(valid_linear_gradient)
                && node
                    .radial_gradient
                    .as_ref()
                    .is_none_or(valid_radial_gradient)
                && node
                    .conic_gradient
                    .as_ref()
                    .is_none_or(valid_conic_gradient)
                && [
                    node.linear_gradient.is_some(),
                    node.radial_gradient.is_some(),
                    node.conic_gradient.is_some(),
                ]
                .into_iter()
                .filter(|present| *present)
                .count()
                    <= 1
                && (node.background_layers.is_empty()
                    || (node.linear_gradient.is_none()
                        && node.radial_gradient.is_none()
                        && node.conic_gradient.is_none()))
        })
        && nodes.iter().all(|node| {
            let mut seen = std::collections::BTreeSet::new();
            let mut current = Some(node.id);
            while let Some(id) = current {
                if !seen.insert(id) {
                    return false;
                }
                current = nodes
                    .iter()
                    .find(|candidate| candidate.id == id)
                    .and_then(|candidate| candidate.parent);
            }
            true
        })
}

fn valid_font_tag(tag: &str) -> bool {
    tag.len() == 4 && tag.bytes().all(|byte| (b' '..=b'~').contains(&byte))
}

impl LengthPercentageFixture {
    fn is_finite(&self) -> bool {
        self.length.is_finite() && self.fraction.is_finite()
    }

    fn is_non_negative(&self) -> bool {
        self.is_finite() && self.length >= 0.0 && self.fraction >= 0.0
    }
}

impl PathCommandFixture {
    fn is_valid(&self) -> bool {
        let point = |value: &[LengthPercentageFixture; 2]| {
            value.iter().all(LengthPercentageFixture::is_finite)
        };
        match self {
            Self::MoveTo { point: value } | Self::LineTo { point: value } => point(value),
            Self::QuadraticTo { control, end } => point(control) && point(end),
            Self::CubicTo {
                control_1,
                control_2,
                end,
            } => point(control_1) && point(control_2) && point(end),
            Self::Close => true,
        }
    }
}
