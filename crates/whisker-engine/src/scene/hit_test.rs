use whisker_protocol::{
    ClipShape, FillRule, HitTestBehavior, InputPoint, LayoutRect, NodeId, OverflowClip, PaintBox,
    PaintCoordinate, PaintCornerRadius, PaintCorners, PaintLengthPercentage, PaintPosition,
    PathCommand, Transform, Visibility,
};

use super::{Scene, SceneError, SceneNode};

impl Scene {
    /// Finds the visually topmost node at one surface-space point.
    pub fn hit_test(&self, root: NodeId, point: InputPoint) -> Result<Option<NodeId>, SceneError> {
        self.require_node(root)?;
        Ok(self.hit_test_node(root, point, [0.0; 2]))
    }

    fn hit_test_node(
        &self,
        node: NodeId,
        point: InputPoint,
        parent_origin: [f32; 2],
    ) -> Option<NodeId> {
        let state = self
            .nodes
            .get(&node)
            .expect("hit-test traversal only visits retained nodes");
        if state.hit_test == Some(HitTestBehavior::None) {
            return None;
        }

        let layout = state.layout?;
        let border = LayoutRect {
            x: parent_origin[0] + layout.border_box.x,
            y: parent_origin[1] + layout.border_box.y,
            width: layout.border_box.width,
            height: layout.border_box.height,
        };
        let point = inverse_map_around(state.transform, point, [border.x, border.y])?;
        let contains_x = contains_axis(point.x, border.x, border.width);
        let contains_y = contains_axis(point.y, border.y, border.height);
        let contains = contains_x && contains_y;

        // clip-path applies to the element and its complete subtree. Test it
        // before descending so no per-pointer clip stack needs allocating.
        if !clip_path_contains(state, layout.content_box, border, point) {
            return None;
        }

        let children_clipped = state.clip.is_some_and(|clip| {
            let [clip_x, clip_y] = overflow_clip_contains(state, border, point);
            (clip.horizontal == OverflowClip::Hidden && !clip_x)
                || (clip.vertical == OverflowClip::Hidden && !clip_y)
        });
        if state.hit_test != Some(HitTestBehavior::BoxOnly) && !children_clipped {
            let child_origin = [
                border.x - state.host_scroll_offset[0],
                border.y - state.host_scroll_offset[1],
            ];
            if let Some(target) = self.hit_test_children(state, point, child_origin) {
                return Some(target);
            }
        }

        (state.visibility != Some(Visibility::Hidden)
            && contains
            && state.hit_test != Some(HitTestBehavior::DescendantsOnly))
        .then_some(node)
    }

    fn hit_test_children(
        &self,
        state: &SceneNode,
        point: InputPoint,
        child_origin: [f32; 2],
    ) -> Option<NodeId> {
        let first_z = state
            .children
            .first()
            .and_then(|child| self.nodes.get(child))
            .and_then(|child| child.z_order)
            .unwrap_or(0);
        let uniform_z = state.children.iter().all(|child| {
            self.nodes
                .get(child)
                .and_then(|child| child.z_order)
                .unwrap_or(0)
                == first_z
        });

        // Normal UI trees have one stacking level. This path is allocation-free
        // and returns as soon as the topmost matching child is found.
        if uniform_z {
            return state
                .children
                .iter()
                .rev()
                .find_map(|child| self.hit_test_node(*child, point, child_origin));
        }

        // z-index is uncommon. Traverse once and retain the highest matching
        // sibling instead of allocating and sorting on every pointer move.
        let mut best = None;
        for (index, child) in state.children.iter().copied().enumerate() {
            let Some(target) = self.hit_test_node(child, point, child_origin) else {
                continue;
            };
            let z = self
                .nodes
                .get(&child)
                .and_then(|child| child.z_order)
                .unwrap_or(0);
            if best.is_none_or(|((best_z, best_index), _)| (z, index) > (best_z, best_index)) {
                best = Some(((z, index), target));
            }
        }
        best.map(|(_, target)| target)
    }
}

fn contains_axis(value: f32, start: f32, length: f32) -> bool {
    value >= start && value <= start + length
}

fn inverse_map_around(
    transform: Option<Transform>,
    point: InputPoint,
    origin: [f32; 2],
) -> Option<InputPoint> {
    let Some(transform) = transform.filter(|transform| *transform != Transform::IDENTITY) else {
        return Some(point);
    };
    let [x, y] = [point.x - origin[0], point.y - origin[1]];
    let matrix = transform.0;
    let [a, c, tx] = [matrix[0], matrix[4], matrix[12]];
    let [b, d, ty] = [matrix[1], matrix[5], matrix[13]];
    let [p, q, r] = [matrix[3], matrix[7], matrix[15]];
    let determinant = a * (d * r - ty * q) - c * (b * r - ty * p) + tx * (b * q - d * p);
    if determinant.abs() <= f32::EPSILON {
        return None;
    }

    // The common determinant divisor cancels during the homogeneous divide.
    let inverse_x = (d * r - ty * q) * x + (tx * q - c * r) * y + (c * ty - tx * d);
    let inverse_y = (ty * p - b * r) * x + (a * r - tx * p) * y + (tx * b - a * ty);
    let inverse_w = (b * q - d * p) * x + (c * p - a * q) * y + (a * d - c * b);
    if inverse_w.abs() <= f32::EPSILON {
        return None;
    }
    let mapped = InputPoint {
        x: origin[0] + inverse_x / inverse_w,
        y: origin[1] + inverse_y / inverse_w,
    };
    mapped.is_valid().then_some(mapped)
}

fn overflow_clip_contains(state: &SceneNode, border: LayoutRect, point: InputPoint) -> [bool; 2] {
    let clip = state
        .clip
        .expect("overflow geometry is queried only for a clip");
    let Some(paint) = state.box_paint.as_ref() else {
        return [
            contains_axis(point.x, border.x, border.width),
            contains_axis(point.y, border.y, border.height),
        ];
    };
    let top = resolve_length(paint.border_widths.top, border.height).min(border.height);
    let right = resolve_length(paint.border_widths.right, border.width).min(border.width);
    let bottom = resolve_length(paint.border_widths.bottom, border.height).min(border.height);
    let left = resolve_length(paint.border_widths.left, border.width).min(border.width);
    let inner = LayoutRect {
        x: border.x + left,
        y: border.y + top,
        width: (border.width - left - right).max(0.0),
        height: (border.height - top - bottom).max(0.0),
    };
    if clip.horizontal == OverflowClip::Hidden && clip.vertical == OverflowClip::Hidden {
        let (outer_x, outer_y) = resolve_radii(&paint.border_radii, border);
        let inner_x = [
            (outer_x[0] - left).max(0.0),
            (outer_x[1] - right).max(0.0),
            (outer_x[2] - right).max(0.0),
            (outer_x[3] - left).max(0.0),
        ];
        let inner_y = [
            (outer_y[0] - top).max(0.0),
            (outer_y[1] - top).max(0.0),
            (outer_y[2] - bottom).max(0.0),
            (outer_y[3] - bottom).max(0.0),
        ];
        let contains = rounded_rect_contains(inner, (inner_x, inner_y), point);
        return [contains; 2];
    }
    [
        contains_axis(point.x, inner.x, inner.width),
        contains_axis(point.y, inner.y, inner.height),
    ]
}

fn clip_path_contains(
    state: &SceneNode,
    content: LayoutRect,
    border: LayoutRect,
    point: InputPoint,
) -> bool {
    let Some((reference_box, shape)) = state.visual_effects.clip_path.as_ref() else {
        return true;
    };
    let reference = match reference_box {
        PaintBox::Content => LayoutRect {
            x: border.x + content.x,
            y: border.y + content.y,
            width: content.width,
            height: content.height,
        },
        PaintBox::Padding => padding_box(state, border),
        _ => border,
    };
    shape_contains(shape, reference, point)
}

fn padding_box(state: &SceneNode, border: LayoutRect) -> LayoutRect {
    let Some(paint) = state.box_paint.as_ref() else {
        return border;
    };
    let top = resolve_length(paint.border_widths.top, border.height).min(border.height);
    let right = resolve_length(paint.border_widths.right, border.width).min(border.width);
    let bottom = resolve_length(paint.border_widths.bottom, border.height).min(border.height);
    let left = resolve_length(paint.border_widths.left, border.width).min(border.width);
    LayoutRect {
        x: border.x + left,
        y: border.y + top,
        width: (border.width - left - right).max(0.0),
        height: (border.height - top - bottom).max(0.0),
    }
}

fn shape_contains(shape: &ClipShape, reference: LayoutRect, point: InputPoint) -> bool {
    match shape {
        ClipShape::Inset { edges, radii } => {
            let top = resolve_coordinate(edges.top, reference.height);
            let right = resolve_coordinate(edges.right, reference.width);
            let bottom = resolve_coordinate(edges.bottom, reference.height);
            let left = resolve_coordinate(edges.left, reference.width);
            let rect = LayoutRect {
                x: reference.x + left,
                y: reference.y + top,
                width: (reference.width - left - right).max(0.0),
                height: (reference.height - top - bottom).max(0.0),
            };
            rounded_rect_contains(rect, resolve_radii(radii, rect), point)
        }
        ClipShape::Circle { radius, center } => {
            let center = resolve_position(*center, reference);
            let diagonal = reference.width.hypot(reference.height) / 2.0_f32.sqrt();
            ellipse_contains(center, [resolve_length(*radius, diagonal); 2], point)
        }
        ClipShape::Ellipse {
            radius_x,
            radius_y,
            center,
        } => ellipse_contains(
            resolve_position(*center, reference),
            [
                resolve_length(*radius_x, reference.width),
                resolve_length(*radius_y, reference.height),
            ],
            point,
        ),
        ClipShape::Polygon { fill_rule, points } => {
            let mut winding = Winding::new(*fill_rule, point);
            let Some(first) = points.first().copied() else {
                return false;
            };
            let first = resolve_position(first, reference);
            let mut previous = first;
            for position in &points[1..] {
                let current = resolve_position(*position, reference);
                winding.segment(previous, current);
                previous = current;
            }
            winding.segment(previous, first);
            winding.contains()
        }
        ClipShape::Path {
            fill_rule,
            commands,
        } => path_contains(*fill_rule, commands, reference, point),
    }
}

fn ellipse_contains(center: [f32; 2], radii: [f32; 2], point: InputPoint) -> bool {
    if radii[0] <= 0.0 || radii[1] <= 0.0 {
        return false;
    }
    let x = (point.x - center[0]) / radii[0];
    let y = (point.y - center[1]) / radii[1];
    x * x + y * y <= 1.0
}

fn rounded_rect_contains(rect: LayoutRect, radii: ([f32; 4], [f32; 4]), point: InputPoint) -> bool {
    if !contains_axis(point.x, rect.x, rect.width) || !contains_axis(point.y, rect.y, rect.height) {
        return false;
    }
    let (horizontal, vertical) = radii;
    let corners = [
        (
            0,
            [rect.x + horizontal[0], rect.y + vertical[0]],
            -1.0,
            -1.0,
        ),
        (
            1,
            [rect.x + rect.width - horizontal[1], rect.y + vertical[1]],
            1.0,
            -1.0,
        ),
        (
            2,
            [
                rect.x + rect.width - horizontal[2],
                rect.y + rect.height - vertical[2],
            ],
            1.0,
            1.0,
        ),
        (
            3,
            [rect.x + horizontal[3], rect.y + rect.height - vertical[3]],
            -1.0,
            1.0,
        ),
    ];
    for (index, center, x_sign, y_sign) in corners {
        let in_x_corner = (point.x - center[0]) * x_sign > 0.0;
        let in_y_corner = (point.y - center[1]) * y_sign > 0.0;
        if in_x_corner && in_y_corner {
            return ellipse_contains(center, [horizontal[index], vertical[index]], point);
        }
    }
    true
}

fn resolve_radii(
    radii: &PaintCorners<PaintCornerRadius>,
    rect: LayoutRect,
) -> ([f32; 4], [f32; 4]) {
    let values = [
        radii.top_left,
        radii.top_right,
        radii.bottom_right,
        radii.bottom_left,
    ];
    let mut horizontal = values.map(|radius| resolve_length(radius.horizontal, rect.width));
    let mut vertical = values.map(|radius| resolve_length(radius.vertical, rect.height));
    let scale = [
        radius_scale(rect.width, horizontal[0] + horizontal[1]),
        radius_scale(rect.width, horizontal[3] + horizontal[2]),
        radius_scale(rect.height, vertical[0] + vertical[3]),
        radius_scale(rect.height, vertical[1] + vertical[2]),
    ]
    .into_iter()
    .fold(1.0_f32, f32::min);
    for value in &mut horizontal {
        *value *= scale;
    }
    for value in &mut vertical {
        *value *= scale;
    }
    (horizontal, vertical)
}

fn radius_scale(available: f32, required: f32) -> f32 {
    if required > available && required > 0.0 {
        available / required
    } else {
        1.0
    }
}

fn resolve_length(value: PaintLengthPercentage, available: f32) -> f32 {
    value.length + value.fraction * available
}

fn resolve_coordinate(value: PaintCoordinate, available: f32) -> f32 {
    value.length + value.fraction * available
}

fn resolve_position(position: PaintPosition, reference: LayoutRect) -> [f32; 2] {
    [
        reference.x + resolve_coordinate(position.x, reference.width),
        reference.y + resolve_coordinate(position.y, reference.height),
    ]
}

struct Winding {
    rule: FillRule,
    point: InputPoint,
    winding: i32,
    parity: bool,
}

impl Winding {
    const fn new(rule: FillRule, point: InputPoint) -> Self {
        Self {
            rule,
            point,
            winding: 0,
            parity: false,
        }
    }

    fn segment(&mut self, from: [f32; 2], to: [f32; 2]) {
        let crosses = (from[1] <= self.point.y && to[1] > self.point.y)
            || (from[1] > self.point.y && to[1] <= self.point.y);
        if !crosses {
            return;
        }
        let intersection =
            from[0] + (self.point.y - from[1]) * (to[0] - from[0]) / (to[1] - from[1]);
        if intersection <= self.point.x {
            return;
        }
        self.parity = !self.parity;
        self.winding += if to[1] > from[1] { 1 } else { -1 };
    }

    const fn contains(&self) -> bool {
        match self.rule {
            FillRule::NonZero => self.winding != 0,
            FillRule::EvenOdd => self.parity,
        }
    }
}

fn path_contains(
    fill_rule: FillRule,
    commands: &[PathCommand],
    reference: LayoutRect,
    point: InputPoint,
) -> bool {
    const CURVE_STEPS: usize = 16;
    let mut winding = Winding::new(fill_rule, point);
    let mut current = None;
    let mut start = None;
    let close = |winding: &mut Winding, current: &mut Option<[f32; 2]>, start| {
        if let (Some(from), Some(to)) = (*current, start) {
            winding.segment(from, to);
            *current = Some(to);
        }
    };
    for command in commands {
        match command {
            PathCommand::MoveTo(position) => {
                close(&mut winding, &mut current, start);
                let next = resolve_position(*position, reference);
                current = Some(next);
                start = Some(next);
            }
            PathCommand::LineTo(position) => {
                let next = resolve_position(*position, reference);
                if let Some(previous) = current {
                    winding.segment(previous, next);
                }
                current = Some(next);
            }
            PathCommand::QuadraticTo { control, end } => {
                let Some(from) = current else { continue };
                let control = resolve_position(*control, reference);
                let end = resolve_position(*end, reference);
                let mut previous = from;
                for step in 1..=CURVE_STEPS {
                    let t = step as f32 / CURVE_STEPS as f32;
                    let inverse = 1.0 - t;
                    let next = [
                        inverse * inverse * from[0]
                            + 2.0 * inverse * t * control[0]
                            + t * t * end[0],
                        inverse * inverse * from[1]
                            + 2.0 * inverse * t * control[1]
                            + t * t * end[1],
                    ];
                    winding.segment(previous, next);
                    previous = next;
                }
                current = Some(end);
            }
            PathCommand::CubicTo {
                control_1,
                control_2,
                end,
            } => {
                let Some(from) = current else { continue };
                let control_1 = resolve_position(*control_1, reference);
                let control_2 = resolve_position(*control_2, reference);
                let end = resolve_position(*end, reference);
                let mut previous = from;
                for step in 1..=CURVE_STEPS {
                    let t = step as f32 / CURVE_STEPS as f32;
                    let inverse = 1.0 - t;
                    let next = [
                        inverse.powi(3) * from[0]
                            + 3.0 * inverse * inverse * t * control_1[0]
                            + 3.0 * inverse * t * t * control_2[0]
                            + t.powi(3) * end[0],
                        inverse.powi(3) * from[1]
                            + 3.0 * inverse * inverse * t * control_1[1]
                            + 3.0 * inverse * t * t * control_2[1]
                            + t.powi(3) * end[1],
                    ];
                    winding.segment(previous, next);
                    previous = next;
                }
                current = Some(end);
            }
            PathCommand::Close => close(&mut winding, &mut current, start),
        }
    }
    close(&mut winding, &mut current, start);
    winding.contains()
}

#[cfg(test)]
mod tests {
    use whisker_protocol::{
        BorderLineStyle, BoxClip, BoxPaint, ElementTypeId, LayoutRect, PaintColor, PaintEdges,
        SurfaceId, VisualEffects,
    };

    use super::*;

    fn scene_with_child() -> (Scene, NodeId, NodeId) {
        let mut scene = Scene::new(SurfaceId::new(1).unwrap());
        let element = ElementTypeId::new(1).unwrap();
        let root = scene.create_node(element).unwrap();
        let child = scene.create_node(element).unwrap();
        scene.insert_child(root, child, 0).unwrap();
        scene
            .set_layout(
                root,
                LayoutRect {
                    x: 0.0,
                    y: 0.0,
                    width: 300.0,
                    height: 300.0,
                },
            )
            .unwrap();
        scene
            .set_layout(
                child,
                LayoutRect {
                    x: 0.0,
                    y: 0.0,
                    width: 50.0,
                    height: 50.0,
                },
            )
            .unwrap();
        (scene, root, child)
    }

    fn length(value: f32) -> PaintLengthPercentage {
        PaintLengthPercentage {
            length: value,
            fraction: 0.0,
        }
    }

    fn coordinate(x: f32, y: f32) -> PaintPosition {
        PaintPosition {
            x: PaintCoordinate {
                length: x,
                fraction: 0.0,
            },
            y: PaintCoordinate {
                length: y,
                fraction: 0.0,
            },
        }
    }

    fn radii(value: f32) -> PaintCorners<PaintCornerRadius> {
        let radius = PaintCornerRadius::circular(length(value));
        PaintCorners {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        }
    }

    fn bordered_paint(width: f32, radius: f32) -> BoxPaint {
        let transparent = PaintColor::Named("transparent".into());
        BoxPaint {
            background_color: transparent.clone(),
            border_widths: PaintEdges {
                top: length(width),
                right: length(width),
                bottom: length(width),
                left: length(width),
            },
            border_colors: PaintEdges {
                top: transparent.clone(),
                right: transparent.clone(),
                bottom: transparent.clone(),
                left: transparent,
            },
            border_styles: PaintEdges {
                top: BorderLineStyle::None,
                right: BorderLineStyle::None,
                bottom: BorderLineStyle::None,
                left: BorderLineStyle::None,
            },
            border_radii: radii(radius),
        }
    }

    #[test]
    fn transforms_hit_geometry_and_hidden_parents_keep_visible_descendants() {
        let (mut scene, root, child) = scene_with_child();
        let mut translated = Transform::IDENTITY;
        translated.0[12] = 100.0;
        scene.set_transform(child, translated).unwrap();

        assert_eq!(
            scene.hit_test(root, InputPoint { x: 110.0, y: 10.0 }),
            Ok(Some(child))
        );
        assert_eq!(
            scene.hit_test(root, InputPoint { x: 10.0, y: 10.0 }),
            Ok(Some(root))
        );

        scene.set_visibility(root, Visibility::Hidden).unwrap();
        scene.set_visibility(child, Visibility::Visible).unwrap();
        assert_eq!(
            scene.hit_test(root, InputPoint { x: 110.0, y: 10.0 }),
            Ok(Some(child))
        );
    }

    #[test]
    fn host_scroll_offset_tracks_native_presentation_without_dirtying_a_frame() {
        let (mut scene, root, child) = scene_with_child();
        assert_eq!(
            scene.update_host_scroll_offset(root, [f32::NAN, 0.0]),
            Err(SceneError::NonFiniteNumber)
        );
        scene
            .set_layout(
                child,
                LayoutRect {
                    x: 0.0,
                    y: 120.0,
                    width: 50.0,
                    height: 50.0,
                },
            )
            .unwrap();
        scene
            .set_clip(
                root,
                BoxClip {
                    horizontal: OverflowClip::Hidden,
                    vertical: OverflowClip::Hidden,
                },
            )
            .unwrap();
        scene.update_host_scroll_offset(root, [0.0, 100.0]).unwrap();

        assert_eq!(scene.node(root).unwrap().host_scroll_offset(), [0.0, 100.0]);
        assert_eq!(
            scene.hit_test(root, InputPoint { x: 10.0, y: 30.0 }),
            Ok(Some(child))
        );
    }

    #[test]
    fn clip_paths_and_rounded_overflow_restrict_descendant_hits() {
        let (mut scene, root, child) = scene_with_child();
        scene
            .set_layout(
                child,
                LayoutRect {
                    x: 0.0,
                    y: 0.0,
                    width: 300.0,
                    height: 300.0,
                },
            )
            .unwrap();
        let effects = VisualEffects {
            clip_path: Some((
                PaintBox::Border,
                ClipShape::Circle {
                    radius: PaintLengthPercentage {
                        length: 50.0,
                        fraction: 0.0,
                    },
                    center: PaintPosition {
                        x: PaintCoordinate {
                            length: 0.0,
                            fraction: 0.5,
                        },
                        y: PaintCoordinate {
                            length: 0.0,
                            fraction: 0.5,
                        },
                    },
                },
            )),
            ..VisualEffects::default()
        };
        scene.set_visual_effects(root, effects).unwrap();
        assert_eq!(
            scene.hit_test(root, InputPoint { x: 150.0, y: 150.0 }),
            Ok(Some(child))
        );
        assert_eq!(
            scene.hit_test(root, InputPoint { x: 10.0, y: 10.0 }),
            Ok(None)
        );

        scene
            .set_visual_effects(root, VisualEffects::default())
            .unwrap();
        scene
            .set_clip(
                root,
                BoxClip {
                    horizontal: OverflowClip::Hidden,
                    vertical: OverflowClip::Hidden,
                },
            )
            .unwrap();
        let radius = PaintCornerRadius::circular(PaintLengthPercentage {
            length: 100.0,
            fraction: 0.0,
        });
        scene
            .set_box_paint(
                root,
                whisker_protocol::BoxPaint {
                    background_color: PaintColor::Named("transparent".into()),
                    border_widths: PaintEdges {
                        top: PaintLengthPercentage::default(),
                        right: PaintLengthPercentage::default(),
                        bottom: PaintLengthPercentage::default(),
                        left: PaintLengthPercentage::default(),
                    },
                    border_colors: PaintEdges {
                        top: PaintColor::Named("transparent".into()),
                        right: PaintColor::Named("transparent".into()),
                        bottom: PaintColor::Named("transparent".into()),
                        left: PaintColor::Named("transparent".into()),
                    },
                    border_styles: PaintEdges {
                        top: whisker_protocol::BorderLineStyle::None,
                        right: whisker_protocol::BorderLineStyle::None,
                        bottom: whisker_protocol::BorderLineStyle::None,
                        left: whisker_protocol::BorderLineStyle::None,
                    },
                    border_radii: PaintCorners {
                        top_left: radius,
                        top_right: radius,
                        bottom_right: radius,
                        bottom_left: radius,
                    },
                },
            )
            .unwrap();
        assert_eq!(
            scene.hit_test(root, InputPoint { x: 10.0, y: 10.0 }),
            Ok(Some(root))
        );
    }

    #[test]
    fn inverse_mapping_rejects_singular_and_projective_horizon_matrices() {
        let point = InputPoint { x: 1.0, y: 1.0 };
        assert_eq!(
            inverse_map_around(Some(Transform([0.0; 16])), point, [0.0; 2]),
            None
        );

        let mut projective = Transform::IDENTITY;
        projective.0[3] = 1.0;
        assert_eq!(inverse_map_around(Some(projective), point, [0.0; 2]), None);
        assert_eq!(inverse_map_around(None, point, [0.0; 2]), Some(point));
        assert_eq!(
            inverse_map_around(Some(Transform::IDENTITY), point, [0.0; 2]),
            Some(point)
        );

        let (mut scene, root, _) = scene_with_child();
        scene.set_transform(root, Transform([0.0; 16])).unwrap();
        assert_eq!(scene.hit_test(root, point), Ok(None));
    }

    #[test]
    fn clip_reference_boxes_and_shape_families_are_resolved_in_rust() {
        let (mut scene, root, _) = scene_with_child();
        let border = LayoutRect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 80.0,
        };
        let content = LayoutRect {
            x: 10.0,
            y: 10.0,
            width: 80.0,
            height: 60.0,
        };
        assert_eq!(padding_box(scene.node(root).unwrap(), border), border);

        scene
            .set_box_paint(root, bordered_paint(5.0, 10.0))
            .unwrap();
        assert_eq!(
            padding_box(scene.node(root).unwrap(), border),
            LayoutRect {
                x: 15.0,
                y: 25.0,
                width: 90.0,
                height: 70.0,
            }
        );

        for reference_box in [PaintBox::Content, PaintBox::Padding] {
            scene
                .set_visual_effects(
                    root,
                    VisualEffects {
                        clip_path: Some((
                            reference_box,
                            ClipShape::Ellipse {
                                radius_x: length(100.0),
                                radius_y: length(100.0),
                                center: coordinate(50.0, 40.0),
                            },
                        )),
                        ..VisualEffects::default()
                    },
                )
                .unwrap();
            assert!(clip_path_contains(
                scene.node(root).unwrap(),
                content,
                border,
                InputPoint { x: 60.0, y: 60.0 },
            ));
        }

        let reference = LayoutRect {
            width: 100.0,
            height: 100.0,
            ..LayoutRect::default()
        };
        let point = InputPoint { x: 50.0, y: 50.0 };
        assert!(shape_contains(
            &ClipShape::Inset {
                edges: PaintEdges {
                    top: PaintCoordinate::default(),
                    right: PaintCoordinate::default(),
                    bottom: PaintCoordinate::default(),
                    left: PaintCoordinate::default(),
                },
                radii: radii(0.0),
            },
            reference,
            point,
        ));
        assert!(shape_contains(
            &ClipShape::Ellipse {
                radius_x: length(50.0),
                radius_y: length(25.0),
                center: coordinate(50.0, 50.0),
            },
            reference,
            point,
        ));
        assert!(!shape_contains(
            &ClipShape::Polygon {
                fill_rule: FillRule::NonZero,
                points: Vec::new(),
            },
            reference,
            point,
        ));
        let square = vec![
            coordinate(0.0, 0.0),
            coordinate(100.0, 0.0),
            coordinate(100.0, 100.0),
            coordinate(0.0, 100.0),
        ];
        for fill_rule in [FillRule::NonZero, FillRule::EvenOdd] {
            assert!(shape_contains(
                &ClipShape::Polygon {
                    fill_rule,
                    points: square.clone(),
                },
                reference,
                point,
            ));
        }
        assert!(!ellipse_contains([0.0, 0.0], [0.0, 1.0], point));
    }

    #[test]
    fn path_hit_testing_flattens_lines_and_curves() {
        let reference = LayoutRect {
            width: 100.0,
            height: 100.0,
            ..LayoutRect::default()
        };
        let inside = InputPoint { x: 50.0, y: 50.0 };
        assert!(!path_contains(
            FillRule::NonZero,
            &[PathCommand::QuadraticTo {
                control: coordinate(1.0, 1.0),
                end: coordinate(2.0, 2.0),
            }],
            reference,
            inside,
        ));
        assert!(!path_contains(
            FillRule::NonZero,
            &[PathCommand::CubicTo {
                control_1: coordinate(1.0, 1.0),
                control_2: coordinate(2.0, 2.0),
                end: coordinate(3.0, 3.0),
            }],
            reference,
            inside,
        ));
        assert!(!path_contains(
            FillRule::NonZero,
            &[PathCommand::LineTo(coordinate(1.0, 1.0))],
            reference,
            inside,
        ));

        let curved_box = ClipShape::Path {
            fill_rule: FillRule::EvenOdd,
            commands: vec![
                PathCommand::MoveTo(coordinate(0.0, 0.0)),
                PathCommand::MoveTo(coordinate(0.0, 0.0)),
                PathCommand::LineTo(coordinate(100.0, 0.0)),
                PathCommand::QuadraticTo {
                    control: coordinate(100.0, 50.0),
                    end: coordinate(100.0, 100.0),
                },
                PathCommand::CubicTo {
                    control_1: coordinate(50.0, 100.0),
                    control_2: coordinate(0.0, 100.0),
                    end: coordinate(0.0, 0.0),
                },
                PathCommand::Close,
            ],
        };
        assert!(shape_contains(&curved_box, reference, inside));
    }

    #[test]
    fn winding_rounding_and_one_axis_overflow_cover_edge_geometry() {
        let point = InputPoint { x: 0.0, y: 0.0 };
        let mut winding = Winding::new(FillRule::NonZero, point);
        winding.segment([-2.0, -2.0], [-2.0, 2.0]);
        winding.segment([-2.0, 2.0], [2.0, 2.0]);
        winding.segment([2.0, 2.0], [2.0, -2.0]);
        assert!(winding.contains());
        winding.segment([2.0, -2.0], [-2.0, -2.0]);
        assert!(winding.contains());
        let mut even_odd = Winding::new(FillRule::EvenOdd, point);
        even_odd.segment([2.0, -2.0], [2.0, 2.0]);
        assert!(even_odd.contains());

        let rect = LayoutRect {
            width: 100.0,
            height: 100.0,
            ..LayoutRect::default()
        };
        assert!(!rounded_rect_contains(
            rect,
            ([10.0; 4], [10.0; 4]),
            InputPoint { x: -1.0, y: 50.0 },
        ));
        for point in [
            InputPoint { x: 1.0, y: 1.0 },
            InputPoint { x: 99.0, y: 1.0 },
            InputPoint { x: 99.0, y: 99.0 },
            InputPoint { x: 1.0, y: 99.0 },
        ] {
            assert!(!rounded_rect_contains(rect, ([10.0; 4], [10.0; 4]), point));
        }
        assert!(rounded_rect_contains(
            rect,
            ([10.0; 4], [10.0; 4]),
            InputPoint { x: 50.0, y: 50.0 },
        ));
        let (horizontal, vertical) = resolve_radii(&radii(80.0), rect);
        assert_eq!(horizontal, [50.0; 4]);
        assert_eq!(vertical, [50.0; 4]);
        assert_eq!(radius_scale(0.0, 0.0), 1.0);

        let (mut scene, root, _) = scene_with_child();
        scene.set_box_paint(root, bordered_paint(5.0, 0.0)).unwrap();
        scene
            .set_clip(
                root,
                BoxClip {
                    horizontal: OverflowClip::Hidden,
                    vertical: OverflowClip::Visible,
                },
            )
            .unwrap();
        assert_eq!(
            overflow_clip_contains(
                scene.node(root).unwrap(),
                rect,
                InputPoint { x: 50.0, y: 50.0 },
            ),
            [true, true]
        );
    }
}
