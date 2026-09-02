use super::*;

impl DesktopScene {
    #[cfg(test)]
    pub(crate) fn new(surface: SurfaceId, elements: DesktopElementRegistry) -> Self {
        Self::new_with_wake(surface, elements, RuntimeWakeHandle::new(|| {}))
    }

    pub(crate) fn new_with_wake(
        surface: SurfaceId,
        elements: DesktopElementRegistry,
        event_wake: RuntimeWakeHandle,
    ) -> Self {
        Self {
            validation: SceneProjection::new(surface),
            elements,
            nodes: HashMap::new(),
            smooth_scrolls: HashMap::new(),
            pointer_captures: HashMap::new(),
            presentation_pool: HashMap::new(),
            pending_events: Arc::new(Mutex::new(Vec::new())),
            event_wake,
            raster_resources: HashSet::new(),
        }
    }

    pub(crate) fn register_raster_resource(&mut self, resource: ResourceId) {
        self.raster_resources.insert(resource);
    }

    pub(crate) fn release_raster_resource(&mut self, resource: ResourceId) {
        self.raster_resources.remove(&resource);
    }

    pub(crate) fn take_events(&mut self) -> Vec<DesktopProviderEvent> {
        let pending = std::mem::take(&mut *self.pending_events.lock().unwrap());
        pending
            .into_iter()
            .filter_map(|event| {
                let state = self.nodes.get(&event.target)?;
                let resolved = self.elements.event(
                    state.element_type,
                    event.target,
                    &event.name,
                    &event.detail,
                );
                debug_assert!(resolved.is_ok(), "native element emitted invalid event");
                let (name, mask) = resolved.ok()?;
                (state.event_mask & mask != 0).then_some(DesktopProviderEvent {
                    target: event.target,
                    name,
                    detail: event.detail,
                })
            })
            .collect()
    }

    pub(crate) fn cursor_at(&self, point: [f32; 2]) -> Option<whisker_protocol::CursorKeyword> {
        let node = self.hit_test(point)?;
        Some(
            self.nodes
                .get(&node)
                .expect("hit-tested Desktop node remains live")
                .presentation
                .cursor
                .fallback,
        )
    }

    pub(crate) fn hit_test(&self, point: [f32; 2]) -> Option<NodeId> {
        let mut roots = self
            .nodes
            .iter()
            .filter_map(|(node, state)| {
                state
                    .presentation
                    .parent
                    .is_none()
                    .then_some((*node, state.presentation.z_order))
            })
            .collect::<Vec<_>>();
        roots.sort_by_key(|(node, z_order)| (*z_order, node.get()));
        roots
            .into_iter()
            .rev()
            .find_map(|(node, _)| self.hit_test_node(node, point, [0.0, 0.0]))
    }

    pub(crate) fn focus_text_input_at(&mut self, point: [f32; 2]) -> bool {
        let mut target = self.hit_test(point);
        while let Some(node) = target {
            let state = self
                .nodes
                .get(&node)
                .expect("hit-tested Desktop node remains live");
            if state.content.accepts_text_input() {
                break;
            }
            target = state.presentation.parent;
        }
        let mut changed = false;
        for (node, state) in &mut self.nodes {
            if !state.content.accepts_text_input() {
                continue;
            }
            let focused = Some(*node) == target;
            if state.content.text_input_focused() != focused {
                state.content.set_text_input_focus(focused);
                changed = true;
            }
        }
        changed
    }

    pub(crate) fn dispatch_text_input(&mut self, event: &DesktopTextInputEvent) -> bool {
        let Some(node) = self
            .nodes
            .iter()
            .find_map(|(node, state)| state.content.text_input_focused().then_some(*node))
        else {
            return false;
        };
        self.nodes
            .get_mut(&node)
            .expect("focused Desktop editor remains live")
            .content
            .handle_text_input(event);
        true
    }

    pub(crate) fn selected_text(&self) -> Option<String> {
        self.nodes.values().find_map(|state| {
            state
                .content
                .text_input_focused()
                .then(|| state.content.selected_text())
                .flatten()
        })
    }

    pub(crate) fn focused_text_input_rect(&self) -> Option<LayoutRect> {
        let node = self
            .nodes
            .iter()
            .find_map(|(node, state)| state.content.text_input_focused().then_some(*node))?;
        self.absolute_border_box(node)
    }

    fn absolute_border_box(&self, node: NodeId) -> Option<LayoutRect> {
        let state = self.nodes.get(&node)?;
        let local = state.presentation.layout.border_box;
        let Some(parent) = state.presentation.parent else {
            return Some(local);
        };
        let parent_state = self.nodes.get(&parent)?;
        let parent_rect = self.absolute_border_box(parent)?;
        let scroll = if parent_state.content.is_scroll_container() {
            parent_state.scroll_offset
        } else {
            [0.0, 0.0]
        };
        Some(LayoutRect {
            x: parent_rect.x + local.x - scroll[0],
            y: parent_rect.y + local.y - scroll[1],
            width: local.width,
            height: local.height,
        })
    }

    fn hit_test_node(
        &self,
        node: NodeId,
        point: [f32; 2],
        parent_origin: [f32; 2],
    ) -> Option<NodeId> {
        let state = self.nodes.get(&node)?;
        let presentation = &state.presentation;
        if presentation.hit_test == HitTestBehavior::None {
            return None;
        }
        let visible = presentation.visibility == Visibility::Visible;
        let rect = presentation.layout.border_box;
        let origin = [parent_origin[0] + rect.x, parent_origin[1] + rect.y];
        let contains_x = point[0] >= origin[0] && point[0] <= origin[0] + rect.width;
        let contains_y = point[1] >= origin[1] && point[1] <= origin[1] + rect.height;
        let clipped = (presentation.clip.horizontal == OverflowClip::Hidden && !contains_x)
            || (presentation.clip.vertical == OverflowClip::Hidden && !contains_y);
        if presentation.hit_test != HitTestBehavior::BoxOnly && !clipped {
            let child_origin = if state.content.is_scroll_container() {
                [
                    origin[0] - state.scroll_offset[0],
                    origin[1] - state.scroll_offset[1],
                ]
            } else {
                origin
            };
            let mut children = presentation
                .children
                .iter()
                .enumerate()
                .map(|(index, child)| {
                    (
                        *child,
                        self.nodes
                            .get(child)
                            .map_or(0, |child| child.presentation.z_order),
                        index,
                    )
                })
                .collect::<Vec<_>>();
            children.sort_by_key(|(_, z_order, index)| (*z_order, *index));
            for (child, _, _) in children.into_iter().rev() {
                if let Some(target) = self.hit_test_node(child, point, child_origin) {
                    return Some(target);
                }
            }
        }
        (visible
            && contains_x
            && contains_y
            && presentation.hit_test != HitTestBehavior::DescendantsOnly)
            .then_some(node)
    }

    pub(crate) fn scroll_at(&mut self, point: [f32; 2], delta: [f32; 2]) -> bool {
        let Some(node) = self.scroll_node_at(point) else {
            return false;
        };
        if !self.nodes[&node].content.scroll_enabled() {
            return false;
        }
        self.smooth_scrolls.remove(&node);
        let horizontal = self.nodes[&node].content.scroll_horizontal();
        let primary = if horizontal {
            if delta[0].abs() > f32::EPSILON {
                delta[0]
            } else {
                delta[1]
            }
        } else if delta[1].abs() > f32::EPSILON {
            delta[1]
        } else {
            delta[0]
        };
        let applied = if horizontal {
            [primary, 0.0]
        } else {
            [0.0, primary]
        };
        let max = self.max_scroll_offset(node);
        let state = self.nodes.get_mut(&node).expect("scroll hit remains live");
        state
            .scroll_sequence_start
            .get_or_insert(state.scroll_offset);
        let next = [
            (state.scroll_offset[0] + applied[0]).clamp(0.0, max[0]),
            (state.scroll_offset[1] + applied[1]).clamp(0.0, max[1]),
        ];
        let changed = next != state.scroll_offset;
        state.scroll_offset = next;
        if changed {
            self.emit_scroll(node, next, max);
        }
        changed
    }

    fn apply_scroll_command(
        &mut self,
        node: NodeId,
        command: whisker_protocol::CommandId,
        arguments: &WhiskerValue,
    ) -> bool {
        if command != whisker::SCROLL_TO_COMMAND && command != whisker::SCROLL_BY_COMMAND {
            return false;
        }
        let Some(state) = self.nodes.get(&node) else {
            return false;
        };
        if !state.content.is_scroll_container() {
            return false;
        }
        let WhiskerValue::Map(arguments) = arguments else {
            return true;
        };
        let offset = match arguments.get("offset") {
            Some(WhiskerValue::Float(value)) => *value as f32,
            Some(WhiskerValue::Int(value)) => *value as f32,
            _ => 0.0,
        };
        let smooth = matches!(arguments.get("smooth"), Some(WhiskerValue::Bool(true)));
        let horizontal = state.content.scroll_horizontal();
        let axis = usize::from(!horizontal);
        let max = self.max_scroll_offset(node);
        let current = state.scroll_offset[axis];
        let target = if command == whisker::SCROLL_BY_COMMAND {
            current + offset
        } else {
            offset
        }
        .clamp(0.0, max[axis]);
        if (target - current).abs() < f32::EPSILON {
            self.smooth_scrolls.remove(&node);
            return true;
        }
        if smooth {
            let mut target_offset = state.scroll_offset;
            target_offset[axis] = target;
            let distance = (target - current).abs();
            self.smooth_scrolls.insert(
                node,
                SmoothScroll {
                    start: state.scroll_offset,
                    target: target_offset,
                    elapsed_ms: 0.0,
                    duration_ms: (160.0 + distance * 0.35).clamp(180.0, 420.0),
                },
            );
            self.event_wake.wake();
            return true;
        }
        self.smooth_scrolls.remove(&node);
        let state = self.nodes.get_mut(&node).expect("scroll node remains live");
        state.scroll_offset[axis] = target;
        let next = state.scroll_offset;
        self.emit_scroll(node, next, max);
        true
    }

    pub(crate) fn has_active_scroll_animations(&self) -> bool {
        !self.smooth_scrolls.is_empty()
    }

    pub(crate) fn advance_scroll_animations(&mut self, delta_ms: f32) -> bool {
        if self.smooth_scrolls.is_empty() {
            return false;
        }
        let nodes = self.smooth_scrolls.keys().copied().collect::<Vec<_>>();
        for node in nodes {
            let Some(mut animation) = self.smooth_scrolls.get(&node).copied() else {
                continue;
            };
            if !self.nodes.contains_key(&node) {
                self.smooth_scrolls.remove(&node);
                continue;
            }
            animation.elapsed_ms =
                (animation.elapsed_ms + delta_ms.max(0.0)).min(animation.duration_ms);
            let progress = if animation.duration_ms <= f32::EPSILON {
                1.0
            } else {
                animation.elapsed_ms / animation.duration_ms
            };
            let inverse = 1.0 - progress;
            let eased = 1.0 - inverse * inverse * inverse;
            let max = self.max_scroll_offset(node);
            let next = [
                (animation.start[0] + (animation.target[0] - animation.start[0]) * eased)
                    .clamp(0.0, max[0]),
                (animation.start[1] + (animation.target[1] - animation.start[1]) * eased)
                    .clamp(0.0, max[1]),
            ];
            let state = self
                .nodes
                .get_mut(&node)
                .expect("animated scroll node remains live");
            let changed = state.scroll_offset != next;
            state.scroll_offset = next;
            if changed {
                self.emit_scroll(node, next, max);
            }
            if progress >= 1.0 {
                self.smooth_scrolls.remove(&node);
            } else {
                self.smooth_scrolls.insert(node, animation);
            }
        }
        !self.smooth_scrolls.is_empty()
    }

    pub(crate) fn settle_scroll_at(&mut self, point: [f32; 2]) -> bool {
        let Some(node) = self.scroll_node_at(point) else {
            return false;
        };
        self.smooth_scrolls.remove(&node);
        let state = &self.nodes[&node];
        let Some((factor, offset)) = state.content.item_snap() else {
            self.nodes
                .get_mut(&node)
                .expect("scroll hit remains live")
                .scroll_sequence_start = None;
            return false;
        };
        let horizontal = state.content.scroll_horizontal();
        let stop_always = state.content.snap_stop_always();
        let viewport = state.presentation.layout.content_box;
        let current = state.scroll_offset[usize::from(!horizontal)];
        let start =
            state.scroll_sequence_start.unwrap_or(state.scroll_offset)[usize::from(!horizontal)];
        let max = self.max_scroll_offset(node);
        let maximum = max[usize::from(!horizontal)];
        let viewport_extent = if horizontal {
            viewport.width
        } else {
            viewport.height
        };
        let factor = factor.clamp(0.0, 1.0) as f32;
        let offset = offset as f32;
        let mut targets = state
            .presentation
            .children
            .iter()
            .filter_map(|child| self.nodes.get(child))
            .map(|child| {
                let rect = child.presentation.layout.border_box;
                let (start, size) = if horizontal {
                    (rect.x, rect.width)
                } else {
                    (rect.y, rect.height)
                };
                (start + size * factor - viewport_extent * factor + offset).clamp(0.0, maximum)
            })
            .collect::<Vec<_>>();
        targets.sort_by(f32::total_cmp);
        targets.dedup_by(|left, right| (*left - *right).abs() < f32::EPSILON);
        let target = if stop_always && current > start + f32::EPSILON {
            targets
                .iter()
                .copied()
                .find(|target| *target > start + f32::EPSILON)
                .or_else(|| targets.last().copied())
        } else if stop_always && current < start - f32::EPSILON {
            targets
                .iter()
                .rev()
                .copied()
                .find(|target| *target < start - f32::EPSILON)
                .or_else(|| targets.first().copied())
        } else {
            targets
                .into_iter()
                .min_by(|left, right| (left - current).abs().total_cmp(&(right - current).abs()))
        };
        let Some(target) = target else {
            return false;
        };
        let state = self.nodes.get_mut(&node).expect("scroll hit remains live");
        state.scroll_sequence_start = None;
        let mut next = state.scroll_offset;
        next[usize::from(!horizontal)] = target;
        if next == state.scroll_offset {
            return false;
        }
        state.scroll_offset = next;
        self.emit_scroll(node, next, max);
        true
    }

    fn scroll_node_at(&self, point: [f32; 2]) -> Option<NodeId> {
        let mut current = self.hit_test(point);
        while let Some(node) = current {
            let state = self.nodes.get(&node)?;
            if state.content.is_scroll_container() {
                return Some(node);
            }
            current = state.presentation.parent;
        }
        None
    }

    fn emit_scroll(&self, node: NodeId, offset: [f32; 2], max: [f32; 2]) {
        let viewport = self.nodes[&node].presentation.layout.content_box;
        self.pending_events
            .lock()
            .unwrap()
            .push(DesktopProviderEvent {
                target: node,
                name: "scroll".to_owned(),
                detail: WhiskerValue::map([
                    ("scrollLeft", WhiskerValue::Float(f64::from(offset[0]))),
                    ("scrollTop", WhiskerValue::Float(f64::from(offset[1]))),
                    (
                        "scrollWidth",
                        WhiskerValue::Float(f64::from(viewport.width + max[0])),
                    ),
                    (
                        "scrollHeight",
                        WhiskerValue::Float(f64::from(viewport.height + max[1])),
                    ),
                    (
                        "viewportWidth",
                        WhiskerValue::Float(f64::from(viewport.width)),
                    ),
                    (
                        "viewportHeight",
                        WhiskerValue::Float(f64::from(viewport.height)),
                    ),
                ]),
            });
        self.event_wake.wake();
    }

    fn max_scroll_offset(&self, node: NodeId) -> [f32; 2] {
        let state = self.nodes.get(&node).expect("scroll node remains live");
        let viewport = state.presentation.layout.content_box;
        let (content_width, content_height) = state
            .presentation
            .children
            .iter()
            .filter_map(|child| self.nodes.get(child))
            .fold((viewport.width, viewport.height), |extent, child| {
                let rect = child.presentation.layout.border_box;
                (
                    extent.0.max(rect.x + rect.width),
                    extent.1.max(rect.y + rect.height),
                )
            });
        [
            (content_width - viewport.width).max(0.0),
            (content_height - viewport.height).max(0.0),
        ]
    }

    fn clamp_scroll_offsets(&mut self) {
        let scroll_nodes = self
            .nodes
            .iter()
            .filter_map(|(node, state)| state.content.is_scroll_container().then_some(*node))
            .collect::<Vec<_>>();
        for node in scroll_nodes {
            let max = self.max_scroll_offset(node);
            let state = self.nodes.get_mut(&node).expect("scroll node remains live");
            state.scroll_offset[0] = state.scroll_offset[0].clamp(0.0, max[0]);
            state.scroll_offset[1] = state.scroll_offset[1].clamp(0.0, max[1]);
        }
    }

    pub(crate) fn paint_commands(&self) -> Vec<PaintCommand<'_>> {
        let mut roots = self
            .nodes
            .iter()
            .filter_map(|(id, node)| {
                node.presentation
                    .parent
                    .is_none()
                    .then_some((*id, node.presentation.z_order))
            })
            .collect::<Vec<_>>();
        roots.sort_by_key(|(id, z)| (*z, id.get()));
        let mut commands = Vec::new();
        for (root, _) in roots {
            self.collect_commands(root, PresentationContext::default(), &mut commands);
        }
        commands
    }

    fn collect_commands<'a>(
        &'a self,
        id: NodeId,
        context: PresentationContext,
        commands: &mut Vec<PaintCommand<'a>>,
    ) {
        let node = self.nodes.get(&id).expect("retained child remains live");
        let presentation = &node.presentation;
        let visible = presentation.visibility == Visibility::Visible;
        let opacity_group = presentation.opacity < 1.0;
        if opacity_group {
            commands.push(PaintCommand::BeginOpacityGroup {
                node: id,
                opacity: presentation.opacity,
            });
        }
        let border = LayoutRect {
            x: context.origin[0] + presentation.layout.border_box.x,
            y: context.origin[1] + presentation.layout.border_box.y,
            width: presentation.layout.border_box.width,
            height: presentation.layout.border_box.height,
        };
        let opacity = 1.0;
        let content = LayoutRect {
            x: border.x + presentation.layout.content_box.x,
            y: border.y + presentation.layout.content_box.y,
            width: presentation.layout.content_box.width,
            height: presentation.layout.content_box.height,
        };
        let transform = multiply_transform(
            context.transform,
            transform_around(presentation.transform, border.x, border.y),
        );
        let mut node_shape_clips = context.shape_clips.clone();
        let mut node_clip_bounds = None;
        if let Some((reference_box, shape)) = presentation.visual_effects.clip_path.as_ref() {
            let reference = match reference_box {
                PaintBox::Border => border,
                PaintBox::Padding => presentation.paint.as_ref().map_or(border, |paint| {
                    resolve_box_geometry(border, paint).inner_rect
                }),
                PaintBox::Content => content,
                _ => unreachable!("unsupported clip-path reference box passed validation"),
            };
            let (clip_rect, clip_radii, path, fill_rule) = clip_shape_geometry(reference, shape);
            node_shape_clips = node_shape_clips.push(ShapeClip {
                rect: clip_rect,
                radii: clip_radii,
                inverse_transform: inverse_transform(transform).unwrap_or(Transform::IDENTITY),
                horizontal: true,
                vertical: true,
                path,
                fill_rule,
            });
            node_clip_bounds = transform_rect_aabb(clip_rect, transform);
        }
        if visible
            && let Some(radius) = presentation
                .visual_effects
                .backdrop_blur
                .filter(|value| *value > 0.0)
            && let Some(rect) = transform_rect_aabb(border, transform)
        {
            commands.push(PaintCommand::BackdropBlur {
                rect,
                radius,
                clip: context.clip,
            });
        }
        if visible
            && (presentation.paint.is_some()
                || !presentation.background_layers.is_empty()
                || !presentation.visual_effects.box_shadows.is_empty())
        {
            commands.push(PaintCommand::Box {
                rect: border,
                content_rect: content,
                paint: presentation.paint.as_ref(),
                background_layers: &presentation.background_layers,
                visual_effects: &presentation.visual_effects,
                clip: context.clip,
                shape_clips: node_shape_clips.clone(),
                transform,
                opacity,
            });
        }

        let clip_horizontal = presentation.clip.horizontal == OverflowClip::Hidden;
        let clip_vertical = presentation.clip.vertical == OverflowClip::Hidden;
        let mut descendant_clip = context.clip;
        if let Some(bounds) = node_clip_bounds {
            descendant_clip = descendant_clip.intersect(bounds, true, true);
        }
        let mut descendant_shape_clips = node_shape_clips;
        if clip_horizontal || clip_vertical {
            let geometry = presentation.paint.as_ref().map_or_else(
                || crate::paint::box_paint::BoxGeometry {
                    outer_rect: border,
                    outer_radii: ResolvedRadii {
                        horizontal: [0.0; 4],
                        vertical: [0.0; 4],
                    },
                    inner_rect: border,
                    inner_radii: ResolvedRadii {
                        horizontal: [0.0; 4],
                        vertical: [0.0; 4],
                    },
                    border_widths: [0.0; 4],
                },
                |paint| resolve_box_geometry(border, paint),
            );
            descendant_shape_clips = descendant_shape_clips.push(ShapeClip {
                rect: geometry.inner_rect,
                radii: geometry.inner_radii,
                inverse_transform: inverse_transform(transform).unwrap_or(Transform::IDENTITY),
                horizontal: clip_horizontal,
                vertical: clip_vertical,
                path: None,
                fill_rule: FillRule::NonZero,
            });
            if let Some(bounds) = transform_rect_aabb(geometry.inner_rect, transform) {
                let axis_aligned = preserves_screen_axes(transform);
                descendant_clip = descendant_clip.intersect(
                    bounds,
                    clip_horizontal && (clip_vertical || axis_aligned),
                    clip_vertical && (clip_horizontal || axis_aligned),
                );
            }
        }
        if visible && let Some(content) = node.content.text() {
            let content_rect = LayoutRect {
                x: border.x + presentation.layout.content_box.x,
                y: border.y + presentation.layout.content_box.y,
                width: presentation.layout.content_box.width,
                height: presentation.layout.content_box.height,
            };
            commands.push(PaintCommand::Text {
                node: id,
                rect: content_rect,
                content,
                clip: descendant_clip.intersect(content_rect, true, true),
                shape_clips: descendant_shape_clips.clone(),
                transform,
                opacity,
            });
        }
        if visible && let Some(rasterizer) = node.content.rasterizer() {
            commands.push(PaintCommand::Raster {
                node: id,
                rect: content,
                rasterizer,
                clip: descendant_clip.intersect(content, true, true),
                shape_clips: descendant_shape_clips.clone(),
                transform,
                opacity,
            });
        }

        let mut children = node
            .presentation
            .children
            .iter()
            .enumerate()
            .map(|(index, child)| {
                let z = self
                    .nodes
                    .get(child)
                    .expect("retained child remains live")
                    .presentation
                    .z_order;
                (*child, z, index)
            })
            .collect::<Vec<_>>();
        children.sort_by_key(|(_, z, index)| (*z, *index));
        for (child, _, _) in children {
            let child_origin = if node.content.is_scroll_container() {
                [
                    border.x - node.scroll_offset[0],
                    border.y - node.scroll_offset[1],
                ]
            } else {
                [border.x, border.y]
            };
            self.collect_commands(
                child,
                PresentationContext {
                    origin: child_origin,
                    transform,
                    clip: descendant_clip,
                    shape_clips: descendant_shape_clips.clone(),
                },
                commands,
            );
        }
        if opacity_group {
            commands.push(PaintCommand::EndOpacityGroup { node: id });
        }
    }

    fn validate_element_operations(&self, packet: &FramePacket) -> Result<(), DesktopPresentError> {
        let mut types = if packet.header.mode == FrameMode::Snapshot {
            HashMap::new()
        } else {
            self.nodes
                .iter()
                .map(|(node, state)| (*node, state.element_type))
                .collect()
        };
        for operation in &packet.operations {
            match operation {
                Operation::CreateNode { node, element_type } => {
                    self.elements
                        .create(*element_type, DesktopEventEmitter::default())?;
                    types.insert(*node, *element_type);
                }
                Operation::InsertChild { parent, .. } => {
                    if let Some(element_type) = types.get(parent).copied()
                        && !self.elements.child_policy(element_type)?.accepts_elements()
                    {
                        return Err(
                            DesktopElementError::ChildrenNotAllowed { parent: *parent }.into()
                        );
                    }
                }
                Operation::SetText { node, content } => {
                    if content.paint.decoration.lines.overline
                        || (content.paint.decoration.lines.underline
                            && content.paint.decoration.lines.line_through)
                        || !matches!(
                            content.paint.decoration.thickness,
                            whisker_protocol::TextDecorationThickness::Auto
                        )
                    {
                        return Err(DesktopPresentError::Unsupported("text-decoration"));
                    }
                    if let Some(element_type) = types.get(node).copied() {
                        self.elements
                            .create(element_type, DesktopEventEmitter::default())?
                            .set_text(*node, content.clone())?;
                    }
                }
                Operation::SetTextStyle { node, style } => {
                    if let Some(element_type) = types.get(node).copied() {
                        if !self.elements.receives_text_style(element_type)? {
                            return Err(DesktopPresentError::Unsupported("text-style"));
                        }
                        self.elements
                            .create(element_type, DesktopEventEmitter::default())?
                            .set_text_style(*node, style)?;
                    }
                }
                Operation::SetProperty {
                    node,
                    property,
                    value,
                } => {
                    if let Some(element_type) = types.get(node).copied() {
                        self.elements.validate_property(
                            element_type,
                            *node,
                            *property,
                            Some(value),
                        )?;
                    }
                }
                Operation::ClearProperty { node, property } => {
                    if let Some(element_type) = types.get(node).copied() {
                        self.elements
                            .validate_property(element_type, *node, *property, None)?;
                    }
                }
                Operation::InvokeCommand {
                    node,
                    command,
                    arguments,
                    ..
                } => {
                    if let Some(element_type) = types.get(node).copied() {
                        self.elements
                            .validate_command(element_type, *node, *command, arguments)?;
                    }
                }
                Operation::SetBackgroundLayers { layers, .. } => {
                    if !layers.iter().all(|layer| {
                        supports_basic_background_layer(layer)
                            && match &layer.image {
                                PaintImage::Resource(resource) => {
                                    self.raster_resources.contains(resource)
                                }
                                _ => true,
                            }
                    }) {
                        return Err(DesktopPresentError::Unsupported("background-layers"));
                    }
                }
                Operation::SetVisualEffects { effects, .. } => {
                    if !supports_visual_effects(effects) {
                        return Err(DesktopPresentError::Unsupported("visual-effects"));
                    }
                }
                Operation::SetCursor { cursor, .. } if !cursor.resources.is_empty() => {
                    return Err(DesktopPresentError::Unsupported("resource-backed cursor"));
                }
                Operation::DeleteNode { .. }
                | Operation::RemoveChild { .. }
                | Operation::MoveChild { .. }
                | Operation::SetLayout { .. }
                | Operation::SetBoxPaint { .. }
                | Operation::SetClip { .. }
                | Operation::SetTransform { .. }
                | Operation::SetOpacity { .. }
                | Operation::SetVisibility { .. }
                | Operation::SetZOrder { .. }
                | Operation::SetAccessibility { .. }
                | Operation::SetEventMask { .. }
                | Operation::SetHitTest { .. }
                | Operation::SetCursor { .. }
                | Operation::SetPointerCapture { .. }
                | Operation::ReleasePointerCapture { .. } => {}
            }
        }
        Ok(())
    }

    fn apply_operations(&mut self, packet: &FramePacket) {
        if packet.header.mode == FrameMode::Snapshot {
            let nodes = std::mem::take(&mut self.nodes);
            for (_, node) in nodes {
                self.recycle_presentation(node);
            }
            self.pending_events.lock().unwrap().clear();
            self.pointer_captures.clear();
        }
        for operation in &packet.operations {
            match operation {
                Operation::CreateNode { node, element_type } => {
                    let pending_events = Arc::clone(&self.pending_events);
                    let event_wake = self.event_wake.clone();
                    let target = *node;
                    let events = DesktopEventEmitter::new(move |event| {
                        pending_events.lock().unwrap().push(DesktopProviderEvent {
                            target,
                            name: event.event,
                            detail: event.detail,
                        });
                        event_wake.wake();
                    });
                    let content = self
                        .presentation_pool
                        .get_mut(element_type)
                        .and_then(Vec::pop)
                        .unwrap_or_else(|| {
                            self.elements
                                .create(*element_type, events)
                                .expect("element operations were validated before commit")
                        });
                    self.nodes.insert(
                        *node,
                        RenderNode {
                            element_type: *element_type,
                            presentation: CommonPresentation::default(),
                            content,
                            event_mask: 0,
                            scroll_offset: [0.0; 2],
                            scroll_sequence_start: None,
                        },
                    );
                }
                Operation::DeleteNode { node } => self.delete_subtree(*node),
                Operation::InsertChild {
                    parent,
                    child,
                    index,
                } => {
                    self.nodes
                        .get_mut(child)
                        .expect("validated child")
                        .presentation
                        .parent = Some(*parent);
                    self.nodes
                        .get_mut(parent)
                        .expect("validated parent")
                        .presentation
                        .children
                        .insert(*index as usize, *child);
                }
                Operation::RemoveChild { parent, child } => {
                    self.nodes
                        .get_mut(child)
                        .expect("validated child")
                        .presentation
                        .parent = None;
                    self.nodes
                        .get_mut(parent)
                        .expect("validated parent")
                        .presentation
                        .children
                        .retain(|candidate| candidate != child);
                }
                Operation::MoveChild {
                    parent,
                    child,
                    index,
                } => {
                    let children = &mut self
                        .nodes
                        .get_mut(parent)
                        .expect("validated parent")
                        .presentation
                        .children;
                    let old = children
                        .iter()
                        .position(|candidate| candidate == child)
                        .expect("validated direct child");
                    children.remove(old);
                    children.insert(*index as usize, *child);
                }
                Operation::SetLayout { node, geometry } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .layout = *geometry;
                }
                Operation::SetBoxPaint { node, paint } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .paint = Some(paint.clone());
                }
                Operation::SetBackgroundLayers { node, layers } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .background_layers = layers.clone();
                }
                Operation::SetVisualEffects { node, effects } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .visual_effects = effects.clone();
                }
                Operation::SetClip { node, clip } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .clip = *clip;
                }
                Operation::SetTransform { node, transform } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .transform = *transform;
                }
                Operation::SetOpacity { node, opacity } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .opacity = *opacity;
                }
                Operation::SetVisibility { node, visibility } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .visibility = *visibility;
                }
                Operation::SetZOrder { node, z_order } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .z_order = *z_order;
                }
                Operation::SetText { node, content } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .content
                        .set_text(*node, content.clone())
                        .expect("element content operation was validated before commit");
                }
                Operation::SetTextStyle { node, style } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .content
                        .set_text_style(*node, style)
                        .expect("text-style operation was validated before commit");
                }
                Operation::SetAccessibility {
                    node,
                    accessibility,
                } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .accessibility = accessibility.clone();
                }
                Operation::SetProperty {
                    node,
                    property,
                    value,
                } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .content
                        .set_property(*node, *property, value)
                        .expect("element property was validated before commit");
                }
                Operation::ClearProperty { node, property } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .content
                        .clear_property(*node, *property)
                        .expect("element property was validated before commit");
                }
                Operation::SetEventMask { node, event_mask } => {
                    self.nodes.get_mut(node).expect("validated node").event_mask = *event_mask;
                }
                Operation::InvokeCommand {
                    node,
                    command,
                    arguments,
                    ..
                } => {
                    if !self.apply_scroll_command(*node, *command, arguments) {
                        let is_focus = self
                            .nodes
                            .get(node)
                            .and_then(|state| {
                                self.elements.command_name(state.element_type, *command)
                            })
                            .as_deref()
                            == Some("focus");
                        if is_focus {
                            for (candidate, state) in &mut self.nodes {
                                if candidate != node && state.content.text_input_focused() {
                                    state.content.set_text_input_focus(false);
                                }
                            }
                        }
                        let state = self.nodes.get_mut(node).expect("validated node");
                        state
                            .content
                            .invoke_command(*node, *command, arguments)
                            .expect("element command was validated before commit");
                    }
                }
                Operation::SetHitTest { node, behavior } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .hit_test = *behavior;
                }
                Operation::SetCursor { node, cursor } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .cursor = cursor.clone();
                }
                Operation::SetPointerCapture { node, pointer } => {
                    self.pointer_captures.insert(*pointer, *node);
                }
                Operation::ReleasePointerCapture { node, pointer } => {
                    if self.pointer_captures.get(pointer) == Some(node) {
                        self.pointer_captures.remove(pointer);
                    }
                }
            }
        }
        self.clamp_scroll_offsets();
    }

    fn delete_subtree(&mut self, node: NodeId) {
        self.smooth_scrolls.remove(&node);
        let Some(removed) = self.nodes.remove(&node) else {
            return;
        };
        self.pointer_captures.retain(|_, target| *target != node);
        if let Some(parent) = removed.presentation.parent
            && let Some(parent) = self.nodes.get_mut(&parent)
        {
            parent
                .presentation
                .children
                .retain(|candidate| *candidate != node);
        }
        let children = removed.presentation.children.clone();
        for child in children {
            self.delete_subtree(child);
        }
        self.recycle_presentation(removed);
    }

    fn recycle_presentation(&mut self, mut node: RenderNode) {
        if !self.elements.is_builtin_presentation(node.element_type) {
            return;
        }
        node.content.reset_for_presentation_reuse();
        let pool = self.presentation_pool.entry(node.element_type).or_default();
        if pool.len() < 256 {
            pool.push(node.content);
        }
    }
}

impl FrameSink for DesktopScene {
    type Error = DesktopPresentError;

    fn capabilities(&self) -> whisker_protocol::RenderCapabilities {
        crate::capabilities::host_capabilities()
    }

    fn present(&mut self, packet: &FramePacket) -> Result<ApplyResult, Self::Error> {
        if let Some(capability) = self.capabilities().first_unsupported(packet) {
            return Err(DesktopPresentError::Unsupported(capability.as_str()));
        }
        if packet.header.mode == FrameMode::Delta
            && (self.validation.scene_epoch() != Some(packet.header.scene_epoch)
                || self.validation.revision() != packet.header.base_revision)
        {
            return self.validation.apply(packet).map_err(Into::into);
        }
        // Element validation is read-only. The reference projection then
        // validates and commits the complete protocol transaction atomically,
        // allowing the retained Desktop tree to update in place without a
        // whole-scene clone on each delta.
        self.validate_element_operations(packet)?;
        let result = self.validation.apply(packet)?;
        if matches!(result, ApplyResult::Accepted { .. }) {
            self.apply_operations(packet);
        }
        Ok(result)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DesktopPresentError {
    Protocol(ValidationError),
    Element(DesktopElementError),
    Unsupported(&'static str),
}
