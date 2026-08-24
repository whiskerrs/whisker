//! Retained, renderer-independent layout for Whisker scenes.
//!
//! The public API deliberately contains no Taffy types. The implementation
//! keeps Taffy private so layout inputs and snapshots remain Whisker-owned,
//! versionable contracts.

#![warn(missing_docs)]

use std::{collections::BTreeMap, error::Error, fmt};

use taffy::{
    AlignContent, AlignItems, AvailableSpace as TaffyAvailableSpace, BoxSizing, Dimension,
    Direction, Display, FlexDirection, FlexWrap, GridAutoFlow, GridPlacement, GridTemplateArea,
    GridTemplateAreas, GridTemplateComponent, GridTemplateRepetition, LengthPercentage,
    LengthPercentageAuto, Line, MaxTrackSizingFunction, MinTrackSizingFunction, Position, Rect,
    RepetitionCount, Size, Style, TaffyTree, TrackSizingFunction,
};
pub use whisker_protocol::AvailableSpace;
use whisker_protocol::{LayoutGeometry, LayoutRect, MeasureConstraints, NodeId};
use whisker_style::{
    AlignContentValue, AlignItemsValue, AlignSelfValue, BoxSizingValue, ComputedFlexBasis,
    ComputedGridMaxTrackSizing, ComputedGridMinTrackSizing, ComputedGridTemplate,
    ComputedGridTemplateComponent, ComputedGridTrackSizing, ComputedLayoutStyle,
    ComputedLengthPercentage, ComputedLengthPercentageAuto, ComputedSizeValue, DirectionValue,
    DisplayValue, FlexDirectionValue, FlexWrapValue, GridAutoFlowValue, GridPlacementLineValue,
    GridPlacementValue, GridRepetitionCountValue, GridTemplateAreasValue, JustifyContentValue,
    PositionValue, PropertyImpactSet,
};

/// A width and height in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayoutSize {
    /// Horizontal extent.
    pub width: f32,
    /// Vertical extent.
    pub height: f32,
}

impl LayoutSize {
    /// Creates a logical-pixel size.
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    fn is_valid(self) -> bool {
        self.width.is_finite() && self.height.is_finite() && self.width >= 0.0 && self.height >= 0.0
    }
}

/// Inputs supplied when measuring an intrinsically sized leaf.
pub type MeasureRequest = MeasureConstraints;

/// Supplies intrinsic leaf sizes, normally by asking the Host text or media backend.
pub trait IntrinsicMeasurer {
    /// Measures `node` under the supplied constraints.
    fn measure(&mut self, node: NodeId, request: MeasureRequest) -> LayoutSize;
}

impl<F> IntrinsicMeasurer for F
where
    F: FnMut(NodeId, MeasureRequest) -> LayoutSize,
{
    fn measure(&mut self, node: NodeId, request: MeasureRequest) -> LayoutSize {
        self(node, request)
    }
}

/// A deterministic immutable result of one layout pass.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayoutSnapshot {
    boxes: BTreeMap<NodeId, LayoutGeometry>,
}

impl LayoutSnapshot {
    /// Returns the border box for a node relative to its parent content origin.
    pub fn get(&self, node: NodeId) -> Option<&LayoutGeometry> {
        self.boxes.get(&node)
    }

    /// Iterates in stable node-ID order.
    pub fn iter(&self) -> impl Iterator<Item = (NodeId, &LayoutGeometry)> {
        self.boxes.iter().map(|(node, rect)| (*node, rect))
    }

    /// Returns the number of laid-out nodes.
    pub fn len(&self) -> usize {
        self.boxes.len()
    }

    /// Returns whether the snapshot contains no nodes.
    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty()
    }
}

/// A computed style feature not yet representable by the private backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsupportedLayoutFeature {
    /// An affine value has both non-zero length and percentage components.
    MixedLengthPercentage,
    /// `max-content` as a size value.
    MaxContent,
    /// `min-content` as a size value.
    MinContent,
    /// `fit-content` as a size value.
    FitContent,
    /// `content` as a flex basis.
    ContentFlexBasis,
    /// Viewport-fixed positioning.
    FixedPosition,
    /// Scroll-sticky positioning.
    StickyPosition,
}

/// Failure while mutating or computing a retained layout tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutError {
    /// The external node ID is already live.
    DuplicateNode(NodeId),
    /// The external node ID is not live.
    UnknownNode(NodeId),
    /// A child occurs more than once in a replacement list.
    DuplicateChild {
        /// The node receiving children.
        parent: NodeId,
        /// The repeated child.
        child: NodeId,
    },
    /// A child is still attached to another parent.
    ChildAlreadyAttached {
        /// The requested child.
        child: NodeId,
        /// Its current parent.
        parent: NodeId,
    },
    /// The requested relation would create a cycle.
    TreeCycle {
        /// The requested parent.
        parent: NodeId,
        /// The child that would close the cycle.
        child: NodeId,
    },
    /// The compute root is currently attached below another node.
    RootHasParent {
        /// The requested compute root.
        root: NodeId,
        /// Its current parent.
        parent: NodeId,
    },
    /// A reparent insertion index exceeds the resulting child-list length.
    ChildIndexOutOfBounds {
        /// The destination parent.
        parent: NodeId,
        /// The requested insertion index.
        index: usize,
        /// The largest accepted insertion index.
        max_index: usize,
    },
    /// The viewport is negative or non-finite.
    InvalidViewport,
    /// An intrinsic measurer returned a negative or non-finite size.
    InvalidMeasurement(NodeId),
    /// The style uses a feature that cannot yet be represented faithfully.
    UnsupportedStyle(UnsupportedLayoutFeature),
}

impl fmt::Display for LayoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for LayoutError {}

#[derive(Clone, Debug)]
struct RetainedNode {
    backend: taffy::NodeId,
    style: ComputedLayoutStyle,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    measurable: bool,
}

/// A retained tree that converts Whisker computed styles into layout snapshots.
#[derive(Clone, Debug)]
pub struct LayoutTree {
    backend: TaffyTree<NodeId>,
    surface_root: taffy::NodeId,
    surface_child: Option<NodeId>,
    surface_viewport: Option<LayoutSize>,
    nodes: BTreeMap<NodeId, RetainedNode>,
}

impl Default for LayoutTree {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutTree {
    /// Creates an empty tree. Fractional logical coordinates are preserved.
    pub fn new() -> Self {
        let mut backend = TaffyTree::new();
        backend.disable_rounding();
        let surface_root = backend
            .new_leaf(surface_root_style(LayoutSize::default()))
            .expect("valid private surface-root style");
        Self {
            backend,
            surface_root,
            surface_child: None,
            surface_viewport: None,
            nodes: BTreeMap::new(),
        }
    }

    /// Returns the number of live nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns whether the tree contains no nodes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Returns whether an external node ID is live.
    pub fn contains(&self, node: NodeId) -> bool {
        self.nodes.contains_key(&node)
    }

    /// Validates that a computed style can be represented by this backend.
    ///
    /// This performs the same conversion used by node creation and updates
    /// without changing retained state.
    pub fn validate_style(style: &ComputedLayoutStyle) -> Result<(), LayoutError> {
        convert_style(style).map(drop)
    }

    /// Creates an unattached, initially non-measurable node.
    pub fn create_node(
        &mut self,
        node: NodeId,
        style: ComputedLayoutStyle,
    ) -> Result<(), LayoutError> {
        if self.contains(node) {
            return Err(LayoutError::DuplicateNode(node));
        }
        let converted = convert_style(&style)?;
        let backend = self
            .backend
            .new_leaf(converted)
            .expect("valid private style");
        self.nodes.insert(
            node,
            RetainedNode {
                backend,
                style,
                parent: None,
                children: Vec::new(),
                measurable: false,
            },
        );
        Ok(())
    }

    /// Replaces a node's style and reports the resulting invalidation.
    pub fn update_style(
        &mut self,
        node: NodeId,
        style: ComputedLayoutStyle,
    ) -> Result<PropertyImpactSet, LayoutError> {
        let retained = self
            .nodes
            .get(&node)
            .ok_or(LayoutError::UnknownNode(node))?;
        let impact = style.changes_from(&retained.style);
        if impact.is_empty() {
            return Ok(impact);
        }
        let converted = convert_style(&style)?;
        let backend_node = retained.backend;
        let parent = retained.parent;
        self.backend
            .set_style(backend_node, converted)
            .expect("retained backend node");
        self.nodes.get_mut(&node).expect("checked above").style = style;
        if let Some(parent) = parent {
            self.sync_backend_children(parent);
        }
        Ok(impact)
    }

    /// Marks or unmarks a leaf as requiring intrinsic Host measurement.
    pub fn set_measurable(&mut self, node: NodeId, measurable: bool) -> Result<bool, LayoutError> {
        let retained = self
            .nodes
            .get(&node)
            .ok_or(LayoutError::UnknownNode(node))?;
        if retained.measurable == measurable {
            return Ok(false);
        }
        let backend = retained.backend;
        self.backend
            .set_node_context(backend, measurable.then_some(node))
            .expect("retained backend node");
        self.nodes.get_mut(&node).expect("checked above").measurable = measurable;
        Ok(true)
    }

    /// Invalidates cached intrinsic measurement for a node and its ancestors.
    pub fn invalidate_measurement(&mut self, node: NodeId) -> Result<(), LayoutError> {
        let retained = self
            .nodes
            .get(&node)
            .ok_or(LayoutError::UnknownNode(node))?;
        self.backend
            .mark_dirty(retained.backend)
            .expect("retained backend node");
        Ok(())
    }

    /// Atomically replaces a parent's ordered child list.
    ///
    /// A node attached elsewhere must first be detached by replacing its old
    /// parent's children. This keeps scene mutations explicit and deterministic.
    pub fn set_children(&mut self, parent: NodeId, children: &[NodeId]) -> Result<(), LayoutError> {
        let old_children = self
            .nodes
            .get(&parent)
            .ok_or(LayoutError::UnknownNode(parent))?
            .children
            .clone();
        let mut seen = std::collections::BTreeSet::new();
        for &child in children {
            let retained = self
                .nodes
                .get(&child)
                .ok_or(LayoutError::UnknownNode(child))?;
            if !seen.insert(child) {
                return Err(LayoutError::DuplicateChild { parent, child });
            }
            if child == parent || self.is_ancestor(child, parent) {
                return Err(LayoutError::TreeCycle { parent, child });
            }
            if let Some(attached) = retained.parent
                && attached != parent
            {
                return Err(LayoutError::ChildAlreadyAttached {
                    child,
                    parent: attached,
                });
            }
        }
        if old_children == children {
            return Ok(());
        }
        for child in old_children {
            self.nodes.get_mut(&child).expect("retained child").parent = None;
        }
        self.nodes.get_mut(&parent).expect("checked above").children = children.to_vec();
        for &child in children {
            self.nodes.get_mut(&child).expect("checked above").parent = Some(parent);
        }
        self.sync_backend_children(parent);
        Ok(())
    }

    /// Moves a live node to `index` in a destination parent's child list.
    ///
    /// This is the atomic retained-tree operation for moves emitted by the
    /// scene engine. It also reorders a child within its existing parent.
    pub fn reparent(
        &mut self,
        child: NodeId,
        parent: NodeId,
        index: usize,
    ) -> Result<(), LayoutError> {
        let old_parent = self
            .nodes
            .get(&child)
            .ok_or(LayoutError::UnknownNode(child))?
            .parent;
        let destination = self
            .nodes
            .get(&parent)
            .ok_or(LayoutError::UnknownNode(parent))?;
        if child == parent || self.is_ancestor(child, parent) {
            return Err(LayoutError::TreeCycle { parent, child });
        }
        let max_index = destination.children.len() - usize::from(old_parent == Some(parent));
        if index > max_index {
            return Err(LayoutError::ChildIndexOutOfBounds {
                parent,
                index,
                max_index,
            });
        }

        if let Some(old_parent) = old_parent {
            self.nodes
                .get_mut(&old_parent)
                .expect("retained old parent")
                .children
                .retain(|node| *node != child);
            if old_parent != parent {
                self.sync_backend_children(old_parent);
            }
        }
        self.nodes
            .get_mut(&parent)
            .expect("retained destination")
            .children
            .insert(index, child);
        self.nodes.get_mut(&child).expect("retained child").parent = Some(parent);
        self.sync_backend_children(parent);
        Ok(())
    }

    /// Removes a node and its entire retained subtree.
    pub fn remove_subtree(&mut self, node: NodeId) -> Result<(), LayoutError> {
        let parent = self
            .nodes
            .get(&node)
            .ok_or(LayoutError::UnknownNode(node))?
            .parent;
        if let Some(parent) = parent {
            let parent_node = self.nodes.get_mut(&parent).expect("retained parent");
            parent_node.children.retain(|child| *child != node);
            self.sync_backend_children(parent);
        }
        let mut postorder = Vec::new();
        self.collect_postorder(node, &mut postorder);
        for removed in postorder {
            let retained = self.nodes.remove(&removed).expect("collected live node");
            self.backend
                .remove(retained.backend)
                .expect("retained backend node");
            if self.surface_child == Some(removed) {
                self.surface_child = None;
            }
        }
        Ok(())
    }

    /// Computes a root subtree inside a finite surface viewport and returns its snapshot.
    ///
    /// The viewport is represented by a private flex-column parent. It is not
    /// included in the returned snapshot, but gives the application root normal
    /// child semantics: cross-axis stretching, `flex-grow`, percentages, and
    /// absolute positioning all resolve against the surface without rewriting
    /// the application's own style.
    pub fn compute(
        &mut self,
        root: NodeId,
        viewport: LayoutSize,
        measurer: &mut dyn IntrinsicMeasurer,
    ) -> Result<LayoutSnapshot, LayoutError> {
        if !viewport.is_valid() {
            return Err(LayoutError::InvalidViewport);
        }
        let retained = self
            .nodes
            .get(&root)
            .ok_or(LayoutError::UnknownNode(root))?;
        if let Some(parent) = retained.parent {
            return Err(LayoutError::RootHasParent { root, parent });
        }
        let backend_root = retained.backend;
        if self.surface_viewport != Some(viewport) {
            self.backend
                .set_style(self.surface_root, surface_root_style(viewport))
                .expect("retained surface root");
            self.surface_viewport = Some(viewport);
        }
        if self.surface_child != Some(root) {
            self.backend
                .set_children(self.surface_root, &[backend_root])
                .expect("retained surface and application roots");
            self.surface_child = Some(root);
        }
        let mut invalid_measurement = None;
        self.backend
            .compute_layout_with_measure(
                self.surface_root,
                Size {
                    width: TaffyAvailableSpace::Definite(viewport.width),
                    height: TaffyAvailableSpace::Definite(viewport.height),
                },
                |known, available, _, context, _| {
                    let Some(node) = context.copied() else {
                        return Size::ZERO;
                    };
                    let measured = measurer.measure(
                        node,
                        MeasureRequest {
                            known_dimensions: [known.width, known.height],
                            available_space: [
                                from_taffy_available(available.width),
                                from_taffy_available(available.height),
                            ],
                        },
                    );
                    if !measured.is_valid() {
                        invalid_measurement.get_or_insert(node);
                        Size::ZERO
                    } else {
                        Size {
                            width: measured.width,
                            height: measured.height,
                        }
                    }
                },
            )
            .expect("retained backend root");
        if let Some(node) = invalid_measurement {
            self.backend
                .mark_dirty(backend_root)
                .expect("retained backend root");
            return Err(LayoutError::InvalidMeasurement(node));
        }
        let mut snapshot = LayoutSnapshot::default();
        self.collect_snapshot(root, &mut snapshot);
        Ok(snapshot)
    }

    fn is_ancestor(&self, candidate: NodeId, mut node: NodeId) -> bool {
        while let Some(parent) = self.nodes.get(&node).and_then(|entry| entry.parent) {
            if parent == candidate {
                return true;
            }
            node = parent;
        }
        false
    }

    fn sync_backend_children(&mut self, parent: NodeId) {
        let retained = self.nodes.get(&parent).expect("retained parent");
        let backend_parent = retained.backend;
        let mut children = retained.children.clone();
        children.sort_by_key(|child| self.nodes.get(child).expect("retained child").style.order);
        let backend_children = children
            .iter()
            .map(|child| self.nodes.get(child).expect("retained child").backend)
            .collect::<Vec<_>>();
        self.backend
            .set_children(backend_parent, &backend_children)
            .expect("retained backend nodes");
    }

    fn collect_postorder(&self, node: NodeId, output: &mut Vec<NodeId>) {
        for child in &self.nodes.get(&node).expect("live node").children {
            self.collect_postorder(*child, output);
        }
        output.push(node);
    }

    fn collect_snapshot(&self, node: NodeId, snapshot: &mut LayoutSnapshot) {
        let retained = self.nodes.get(&node).expect("retained snapshot node");
        let layout = self
            .backend
            .layout(retained.backend)
            .expect("retained backend node");
        snapshot.boxes.insert(
            node,
            LayoutGeometry {
                border_box: LayoutRect {
                    x: layout.location.x,
                    y: layout.location.y,
                    width: layout.size.width,
                    height: layout.size.height,
                },
                content_box: LayoutRect {
                    x: layout.border.left + layout.padding.left,
                    y: layout.border.top + layout.padding.top,
                    width: layout.content_box_width().max(0.0),
                    height: layout.content_box_height().max(0.0),
                },
            },
        );
        for child in &retained.children {
            self.collect_snapshot(*child, snapshot);
        }
    }
}

fn surface_root_style(viewport: LayoutSize) -> Style {
    Style {
        display: Display::Flex,
        flex_direction: FlexDirection::Column,
        align_items: Some(AlignItems::STRETCH),
        size: Size {
            width: Dimension::length(viewport.width),
            height: Dimension::length(viewport.height),
        },
        ..Style::default()
    }
}

fn from_taffy_available(value: TaffyAvailableSpace) -> AvailableSpace {
    match value {
        TaffyAvailableSpace::Definite(value) => AvailableSpace::Definite(value),
        TaffyAvailableSpace::MinContent => AvailableSpace::MinContent,
        TaffyAvailableSpace::MaxContent => AvailableSpace::MaxContent,
    }
}

fn convert_style(input: &ComputedLayoutStyle) -> Result<Style, LayoutError> {
    Ok(Style {
        display: match input.display {
            DisplayValue::None => Display::None,
            DisplayValue::Flex | DisplayValue::Linear => Display::Flex,
            DisplayValue::Grid => Display::Grid,
            DisplayValue::Relative => Display::Block,
        },
        position: match input.position {
            PositionValue::Relative => Position::Relative,
            PositionValue::Absolute => Position::Absolute,
            PositionValue::Fixed => return unsupported(UnsupportedLayoutFeature::FixedPosition),
            PositionValue::Sticky => return unsupported(UnsupportedLayoutFeature::StickyPosition),
        },
        direction: match input.direction {
            DirectionValue::Ltr => Direction::Ltr,
            DirectionValue::Rtl => Direction::Rtl,
        },
        box_sizing: match input.box_sizing {
            BoxSizingValue::ContentBox => BoxSizing::ContentBox,
            BoxSizingValue::BorderBox => BoxSizing::BorderBox,
        },
        size: Size {
            width: dimension(input.size.width)?,
            height: dimension(input.size.height)?,
        },
        min_size: Size {
            width: dimension(input.min_size.width)?,
            height: dimension(input.min_size.height)?,
        },
        max_size: Size {
            width: dimension(input.max_size.width)?,
            height: dimension(input.max_size.height)?,
        },
        margin: Rect {
            top: length_auto(input.margin.top)?,
            right: length_auto(input.margin.right)?,
            bottom: length_auto(input.margin.bottom)?,
            left: length_auto(input.margin.left)?,
        },
        padding: Rect {
            top: length(input.padding.top, true)?,
            right: length(input.padding.right, true)?,
            bottom: length(input.padding.bottom, true)?,
            left: length(input.padding.left, true)?,
        },
        border: Rect {
            top: length(input.border.top, true)?,
            right: length(input.border.right, true)?,
            bottom: length(input.border.bottom, true)?,
            left: length(input.border.left, true)?,
        },
        inset: Rect {
            top: length_auto(input.inset.top)?,
            right: length_auto(input.inset.right)?,
            bottom: length_auto(input.inset.bottom)?,
            left: length_auto(input.inset.left)?,
        },
        flex_direction: match input.flex_direction {
            FlexDirectionValue::Row => FlexDirection::Row,
            FlexDirectionValue::RowReverse => FlexDirection::RowReverse,
            FlexDirectionValue::Column => FlexDirection::Column,
            FlexDirectionValue::ColumnReverse => FlexDirection::ColumnReverse,
        },
        flex_wrap: match input.flex_wrap {
            FlexWrapValue::NoWrap => FlexWrap::NoWrap,
            FlexWrapValue::Wrap => FlexWrap::Wrap,
            FlexWrapValue::WrapReverse => FlexWrap::WrapReverse,
        },
        flex_grow: input.flex_grow.get(),
        flex_shrink: input.flex_shrink.get(),
        flex_basis: match input.flex_basis {
            ComputedFlexBasis::Auto => Dimension::auto(),
            ComputedFlexBasis::Content => {
                return unsupported(UnsupportedLayoutFeature::ContentFlexBasis);
            }
            ComputedFlexBasis::Value(value) => dimension_value(value)?,
        },
        justify_content: Some(justify(input.justify_content)),
        align_items: Some(align_items(input.align_items)),
        align_self: match input.align_self {
            AlignSelfValue::Auto => None,
            AlignSelfValue::Stretch => Some(AlignItems::STRETCH),
            AlignSelfValue::FlexStart => Some(AlignItems::FLEX_START),
            AlignSelfValue::FlexEnd => Some(AlignItems::FLEX_END),
            AlignSelfValue::Center => Some(AlignItems::CENTER),
            AlignSelfValue::Baseline => Some(AlignItems::BASELINE),
            AlignSelfValue::Start => Some(AlignItems::START),
            AlignSelfValue::End => Some(AlignItems::END),
        },
        justify_items: input.justify_items.map(align_items),
        justify_self: input.justify_self.map(align_self),
        align_content: Some(align_content(input.align_content)),
        gap: Size {
            width: length(input.gap.width, false)?,
            height: length(input.gap.height, false)?,
        },
        aspect_ratio: input.aspect_ratio.map(|ratio| ratio.get()),
        grid_template_columns: grid_template(&input.grid_template_columns)?,
        grid_template_column_names: input.grid_template_columns.line_names.clone(),
        grid_template_rows: grid_template(&input.grid_template_rows)?,
        grid_template_row_names: input.grid_template_rows.line_names.clone(),
        grid_auto_columns: input
            .grid_auto_columns
            .iter()
            .copied()
            .map(grid_track)
            .collect::<Result<Vec<_>, _>>()?,
        grid_auto_rows: input
            .grid_auto_rows
            .iter()
            .copied()
            .map(grid_track)
            .collect::<Result<Vec<_>, _>>()?,
        grid_auto_flow: match input.grid_auto_flow {
            GridAutoFlowValue::Row => GridAutoFlow::Row,
            GridAutoFlowValue::Column => GridAutoFlow::Column,
            GridAutoFlowValue::RowDense => GridAutoFlow::RowDense,
            GridAutoFlowValue::ColumnDense => GridAutoFlow::ColumnDense,
        },
        grid_template_areas: input.grid_template_areas.as_ref().map(grid_template_areas),
        grid_column: grid_placement_line(&input.grid_column),
        grid_row: grid_placement_line(&input.grid_row),
        ..Style::default()
    })
}

fn align_self(value: AlignSelfValue) -> AlignItems {
    match value {
        AlignSelfValue::Auto | AlignSelfValue::Stretch => AlignItems::STRETCH,
        AlignSelfValue::FlexStart => AlignItems::FLEX_START,
        AlignSelfValue::FlexEnd => AlignItems::FLEX_END,
        AlignSelfValue::Center => AlignItems::CENTER,
        AlignSelfValue::Baseline => AlignItems::BASELINE,
        AlignSelfValue::Start => AlignItems::START,
        AlignSelfValue::End => AlignItems::END,
    }
}

fn grid_template(
    value: &ComputedGridTemplate,
) -> Result<Vec<GridTemplateComponent<String>>, LayoutError> {
    value
        .components
        .iter()
        .map(|component| match component {
            ComputedGridTemplateComponent::Track(track) => {
                grid_track(*track).map(GridTemplateComponent::Single)
            }
            ComputedGridTemplateComponent::Repeat(repetition) => {
                let count = match repetition.count {
                    GridRepetitionCountValue::Count(value) => RepetitionCount::Count(value),
                    GridRepetitionCountValue::AutoFill => RepetitionCount::AutoFill,
                    GridRepetitionCountValue::AutoFit => RepetitionCount::AutoFit,
                };
                let tracks = repetition
                    .tracks
                    .iter()
                    .copied()
                    .map(grid_track)
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(GridTemplateComponent::Repeat(GridTemplateRepetition {
                    count,
                    tracks,
                    line_names: repetition.line_names.clone(),
                }))
            }
        })
        .collect()
}

fn grid_track(value: ComputedGridTrackSizing) -> Result<TrackSizingFunction, LayoutError> {
    Ok(TrackSizingFunction {
        min: match value.min {
            ComputedGridMinTrackSizing::Fixed(value) => grid_min_fixed(value)?,
            ComputedGridMinTrackSizing::MinContent => MinTrackSizingFunction::min_content(),
            ComputedGridMinTrackSizing::MaxContent => MinTrackSizingFunction::max_content(),
            ComputedGridMinTrackSizing::Auto => MinTrackSizingFunction::auto(),
        },
        max: match value.max {
            ComputedGridMaxTrackSizing::Fixed(value) => grid_max_fixed(value)?,
            ComputedGridMaxTrackSizing::MinContent => MaxTrackSizingFunction::min_content(),
            ComputedGridMaxTrackSizing::MaxContent => MaxTrackSizingFunction::max_content(),
            ComputedGridMaxTrackSizing::FitContent(value) => match scalar(value)? {
                Scalar::Length(value) => MaxTrackSizingFunction::fit_content_px(value),
                Scalar::Percent(value) => MaxTrackSizingFunction::fit_content_percent(value),
            },
            ComputedGridMaxTrackSizing::Auto => MaxTrackSizingFunction::auto(),
            ComputedGridMaxTrackSizing::Fraction(value) => MaxTrackSizingFunction::fr(value.get()),
        },
    })
}

fn grid_min_fixed(value: ComputedLengthPercentage) -> Result<MinTrackSizingFunction, LayoutError> {
    scalar(value).map(|value| match value {
        Scalar::Length(value) => MinTrackSizingFunction::length(value),
        Scalar::Percent(value) => MinTrackSizingFunction::percent(value),
    })
}

fn grid_max_fixed(value: ComputedLengthPercentage) -> Result<MaxTrackSizingFunction, LayoutError> {
    scalar(value).map(|value| match value {
        Scalar::Length(value) => MaxTrackSizingFunction::length(value),
        Scalar::Percent(value) => MaxTrackSizingFunction::percent(value),
    })
}

fn grid_template_areas(value: &GridTemplateAreasValue) -> GridTemplateAreas<String> {
    GridTemplateAreas {
        areas: value
            .areas
            .iter()
            .map(|area| GridTemplateArea {
                name: area.name.clone(),
                row_start: area.row_start,
                row_end: area.row_end,
                column_start: area.column_start,
                column_end: area.column_end,
            })
            .collect(),
        row_count: value.row_count,
        column_count: value.column_count,
    }
}

fn grid_placement_line(value: &GridPlacementLineValue) -> Line<GridPlacement<String>> {
    Line {
        start: grid_placement(&value.start),
        end: grid_placement(&value.end),
    }
}

fn grid_placement(value: &GridPlacementValue) -> GridPlacement<String> {
    match value {
        GridPlacementValue::Auto => GridPlacement::Auto,
        GridPlacementValue::Line(value) => GridPlacement::Line((*value).into()),
        GridPlacementValue::NamedLine(name, index) => {
            GridPlacement::NamedLine(name.clone(), *index)
        }
        GridPlacementValue::Span(value) => GridPlacement::Span(*value),
        GridPlacementValue::NamedSpan(name, count) => {
            GridPlacement::NamedSpan(name.clone(), *count)
        }
    }
}

fn unsupported<T>(feature: UnsupportedLayoutFeature) -> Result<T, LayoutError> {
    Err(LayoutError::UnsupportedStyle(feature))
}

fn dimension(value: ComputedSizeValue) -> Result<Dimension, LayoutError> {
    match value {
        ComputedSizeValue::Auto | ComputedSizeValue::None => Ok(Dimension::auto()),
        ComputedSizeValue::Value(value) => dimension_value(value),
        ComputedSizeValue::MaxContent => unsupported(UnsupportedLayoutFeature::MaxContent),
        ComputedSizeValue::MinContent => unsupported(UnsupportedLayoutFeature::MinContent),
        ComputedSizeValue::FitContent(_) => unsupported(UnsupportedLayoutFeature::FitContent),
    }
}

fn dimension_value(value: ComputedLengthPercentage) -> Result<Dimension, LayoutError> {
    scalar(value).map(|value| match value {
        Scalar::Length(value) => Dimension::length(value),
        Scalar::Percent(value) => Dimension::percent(value),
    })
}

fn length_auto(value: ComputedLengthPercentageAuto) -> Result<LengthPercentageAuto, LayoutError> {
    match value {
        ComputedLengthPercentageAuto::Auto => Ok(LengthPercentageAuto::auto()),
        ComputedLengthPercentageAuto::Value(value) => scalar(value).map(|value| match value {
            Scalar::Length(value) => LengthPercentageAuto::length(value),
            Scalar::Percent(value) => LengthPercentageAuto::percent(value),
        }),
    }
}

fn length(
    value: ComputedLengthPercentage,
    clamp_negative: bool,
) -> Result<LengthPercentage, LayoutError> {
    scalar(value).map(|value| match value {
        Scalar::Length(value) => LengthPercentage::length(if clamp_negative {
            value.max(0.0)
        } else {
            value
        }),
        Scalar::Percent(value) => LengthPercentage::percent(if clamp_negative {
            value.max(0.0)
        } else {
            value
        }),
    })
}

enum Scalar {
    Length(f32),
    Percent(f32),
}

fn scalar(value: ComputedLengthPercentage) -> Result<Scalar, LayoutError> {
    match (value.length(), value.fraction()) {
        (length, 0.0) => Ok(Scalar::Length(length)),
        (0.0, fraction) => Ok(Scalar::Percent(fraction)),
        _ => unsupported(UnsupportedLayoutFeature::MixedLengthPercentage),
    }
}

fn align_items(value: AlignItemsValue) -> AlignItems {
    match value {
        AlignItemsValue::Stretch => AlignItems::STRETCH,
        AlignItemsValue::FlexStart => AlignItems::FLEX_START,
        AlignItemsValue::FlexEnd => AlignItems::FLEX_END,
        AlignItemsValue::Center => AlignItems::CENTER,
        AlignItemsValue::Baseline => AlignItems::BASELINE,
        AlignItemsValue::Start => AlignItems::START,
        AlignItemsValue::End => AlignItems::END,
    }
}

fn align_content(value: AlignContentValue) -> AlignContent {
    match value {
        AlignContentValue::Stretch => AlignContent::STRETCH,
        AlignContentValue::FlexStart => AlignContent::FLEX_START,
        AlignContentValue::FlexEnd => AlignContent::FLEX_END,
        AlignContentValue::Center => AlignContent::CENTER,
        AlignContentValue::SpaceBetween => AlignContent::SPACE_BETWEEN,
        AlignContentValue::SpaceAround => AlignContent::SPACE_AROUND,
        AlignContentValue::SpaceEvenly => AlignContent::SPACE_EVENLY,
    }
}

fn justify(value: JustifyContentValue) -> AlignContent {
    match value {
        JustifyContentValue::Stretch => AlignContent::STRETCH,
        JustifyContentValue::FlexStart => AlignContent::FLEX_START,
        JustifyContentValue::FlexEnd => AlignContent::FLEX_END,
        JustifyContentValue::Center => AlignContent::CENTER,
        JustifyContentValue::SpaceBetween => AlignContent::SPACE_BETWEEN,
        JustifyContentValue::SpaceAround => AlignContent::SPACE_AROUND,
        JustifyContentValue::SpaceEvenly => AlignContent::SPACE_EVENLY,
        JustifyContentValue::Start => AlignContent::START,
        JustifyContentValue::End => AlignContent::END,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_style::{
        Axes, ComputedGridTemplate, ComputedGridTrackSizing, Edges, GridPlacementLineValue,
        StyleNumber,
    };

    const MIXED: ComputedLengthPercentage = ComputedLengthPercentage::new(1.0, 0.5);

    fn id(value: u64) -> NodeId {
        NodeId::new(value).expect("non-zero test ID")
    }

    fn sized(width: f32, height: f32) -> ComputedLayoutStyle {
        ComputedLayoutStyle {
            size: Axes {
                width: ComputedSizeValue::Value(ComputedLengthPercentage::new(width, 0.0)),
                height: ComputedSizeValue::Value(ComputedLengthPercentage::new(height, 0.0)),
            },
            ..ComputedLayoutStyle::default()
        }
    }

    fn zero_measure(_: NodeId, _: MeasureRequest) -> LayoutSize {
        LayoutSize::default()
    }

    #[test]
    fn grid_tracks_and_explicit_placement_reach_taffy() {
        let root = id(1);
        let first = id(2);
        let second = id(3);
        let third = id(4);
        let mut tree = LayoutTree::new();
        let root_style = ComputedLayoutStyle {
            display: DisplayValue::Grid,
            size: Axes {
                width: ComputedSizeValue::Value(ComputedLengthPercentage::new(300.0, 0.0)),
                height: ComputedSizeValue::Value(ComputedLengthPercentage::new(100.0, 0.0)),
            },
            grid_template_columns: ComputedGridTemplate::tracks([
                ComputedGridTrackSizing::length(100.0),
                ComputedGridTrackSizing::fraction(1.0),
                ComputedGridTrackSizing::length(50.0),
            ]),
            grid_template_rows: ComputedGridTemplate::tracks([
                ComputedGridTrackSizing::length(40.0),
                ComputedGridTrackSizing::length(60.0),
            ]),
            ..ComputedLayoutStyle::default()
        };
        tree.create_node(root, root_style).unwrap();
        tree.create_node(first, ComputedLayoutStyle::default())
            .unwrap();
        tree.create_node(
            second,
            ComputedLayoutStyle {
                grid_column: GridPlacementLineValue::lines(2, 3),
                grid_row: GridPlacementLineValue::lines(2, 3),
                ..ComputedLayoutStyle::default()
            },
        )
        .unwrap();
        tree.create_node(
            third,
            ComputedLayoutStyle {
                grid_column: GridPlacementLineValue::lines(3, 4),
                grid_row: GridPlacementLineValue::lines(1, 2),
                ..ComputedLayoutStyle::default()
            },
        )
        .unwrap();
        tree.set_children(root, &[first, second, third]).unwrap();

        let snapshot = tree
            .compute(root, LayoutSize::new(300.0, 100.0), &mut zero_measure)
            .unwrap();
        assert_eq!(
            snapshot.get(first).unwrap().border_box,
            LayoutRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            }
        );
        assert_eq!(
            snapshot.get(second).unwrap().border_box,
            LayoutRect {
                x: 100.0,
                y: 40.0,
                width: 150.0,
                height: 60.0,
            }
        );
        assert_eq!(
            snapshot.get(third).unwrap().border_box,
            LayoutRect {
                x: 250.0,
                y: 0.0,
                width: 50.0,
                height: 40.0,
            }
        );
    }

    #[test]
    fn grid_justify_items_and_self_control_inline_alignment() {
        let root = id(1);
        let centered = id(2);
        let ended = id(3);
        let mut tree = LayoutTree::new();
        tree.create_node(
            root,
            ComputedLayoutStyle {
                display: DisplayValue::Grid,
                size: Axes {
                    width: ComputedSizeValue::Value(ComputedLengthPercentage::new(200.0, 0.0)),
                    height: ComputedSizeValue::Value(ComputedLengthPercentage::new(50.0, 0.0)),
                },
                grid_template_columns: ComputedGridTemplate::tracks([
                    ComputedGridTrackSizing::length(100.0),
                    ComputedGridTrackSizing::length(100.0),
                ]),
                grid_template_rows: ComputedGridTemplate::tracks([
                    ComputedGridTrackSizing::length(50.0),
                ]),
                justify_items: Some(AlignItemsValue::Center),
                ..ComputedLayoutStyle::default()
            },
        )
        .unwrap();
        tree.create_node(centered, sized(20.0, 10.0)).unwrap();
        tree.create_node(
            ended,
            ComputedLayoutStyle {
                justify_self: Some(AlignSelfValue::End),
                ..sized(20.0, 10.0)
            },
        )
        .unwrap();
        tree.set_children(root, &[centered, ended]).unwrap();

        let snapshot = tree
            .compute(root, LayoutSize::new(200.0, 50.0), &mut zero_measure)
            .unwrap();
        assert_eq!(snapshot.get(centered).unwrap().border_box.x, 40.0);
        assert_eq!(snapshot.get(ended).unwrap().border_box.x, 180.0);
    }

    fn assert_unsupported(style: ComputedLayoutStyle, feature: UnsupportedLayoutFeature) {
        assert_eq!(
            convert_style(&style),
            Err(LayoutError::UnsupportedStyle(feature))
        );
    }

    #[test]
    fn retained_tree_measures_orders_and_snapshots_fractional_layout() {
        let root = id(1);
        let first = id(2);
        let second = id(3);
        let mut tree = LayoutTree::new();
        let mut root_style = sized(100.5, 40.5);
        root_style.padding.left = ComputedLengthPercentage::new(0.25, 0.0);
        tree.create_node(root, root_style).unwrap();

        let first_style = ComputedLayoutStyle {
            order: 1,
            ..ComputedLayoutStyle::default()
        };
        let second_style = ComputedLayoutStyle {
            order: -1,
            ..ComputedLayoutStyle::default()
        };
        tree.create_node(first, first_style).unwrap();
        tree.create_node(second, second_style).unwrap();
        tree.set_measurable(first, true).unwrap();
        tree.set_measurable(second, true).unwrap();
        tree.set_children(root, &[first, second]).unwrap();

        let mut requests = Vec::new();
        let snapshot = tree
            .compute(root, LayoutSize::new(200.0, 100.0), &mut |node, request| {
                requests.push((node, request));
                LayoutSize::new(10.0, 5.0)
            })
            .unwrap();

        assert_eq!(tree.len(), 3);
        assert!(!tree.is_empty());
        assert!(tree.contains(first));
        assert_eq!(snapshot.len(), 3);
        assert!(!snapshot.is_empty());
        assert_eq!(
            snapshot.iter().map(|(node, _)| node).collect::<Vec<_>>(),
            [root, first, second]
        );
        assert_eq!(snapshot.get(root).unwrap().border_box.width, 100.5);
        assert_eq!(snapshot.get(root).unwrap().content_box.x, 0.25);
        assert_eq!(snapshot.get(root).unwrap().content_box.width, 100.25);
        assert_eq!(snapshot.get(second).unwrap().border_box.x, 0.25);
        assert_eq!(snapshot.get(first).unwrap().border_box.x, 10.25);
        assert!(requests.len() >= 2);
        assert!(requests.iter().any(|(node, _)| *node == first));
        assert!(requests.iter().any(|(node, _)| *node == second));
        assert!(
            requests
                .iter()
                .any(|(_, request)| request.known_dimensions == [None, None])
        );

        let cloned = tree.clone();
        assert_eq!(cloned.len(), tree.len());
    }

    #[test]
    fn measurement_cache_invalidation_and_validation_are_explicit() {
        use std::cell::Cell;

        let root = id(1);
        let mut tree = LayoutTree::default();
        tree.create_node(root, ComputedLayoutStyle::default())
            .unwrap();
        tree.set_measurable(root, true).unwrap();
        tree.set_measurable(root, true).unwrap();

        let calls = Cell::new(0);
        let mut measure = |_: NodeId, _: MeasureRequest| {
            calls.set(calls.get() + 1);
            LayoutSize::new(12.0, 8.0)
        };
        tree.compute(root, LayoutSize::new(100.0, 100.0), &mut measure)
            .unwrap();
        let first_pass_calls = calls.get();
        assert!(first_pass_calls > 0);
        tree.compute(root, LayoutSize::new(100.0, 100.0), &mut measure)
            .unwrap();
        assert_eq!(calls.get(), first_pass_calls);
        tree.invalidate_measurement(root).unwrap();
        tree.compute(root, LayoutSize::new(100.0, 100.0), &mut measure)
            .unwrap();
        let invalidated_pass_calls = calls.get();
        assert!(invalidated_pass_calls > first_pass_calls);

        tree.invalidate_measurement(root).unwrap();
        assert_eq!(
            tree.compute(root, LayoutSize::new(100.0, 100.0), &mut |_, _| {
                LayoutSize::new(f32::NAN, 1.0)
            }),
            Err(LayoutError::InvalidMeasurement(root))
        );
        tree.compute(root, LayoutSize::new(100.0, 100.0), &mut measure)
            .unwrap();
        assert!(calls.get() > invalidated_pass_calls);
        tree.set_measurable(root, false).unwrap();
        tree.set_measurable(root, false).unwrap();
        let snapshot = tree
            .compute(root, LayoutSize::new(100.0, 100.0), &mut zero_measure)
            .unwrap();
        assert_eq!(snapshot.get(root).unwrap().border_box.width, 100.0);
        assert_eq!(snapshot.get(root).unwrap().border_box.height, 0.0);
    }

    #[test]
    fn private_surface_root_supplies_viewport_child_semantics() {
        let root = id(1);
        let mut tree = LayoutTree::new();
        let style = ComputedLayoutStyle {
            flex_grow: StyleNumber::new(1.0),
            ..ComputedLayoutStyle::default()
        };
        tree.create_node(root, style).unwrap();

        let first = tree
            .compute(root, LayoutSize::new(320.0, 240.0), &mut zero_measure)
            .unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(
            first.get(root).unwrap().border_box,
            LayoutRect {
                x: 0.0,
                y: 0.0,
                width: 320.0,
                height: 240.0,
            }
        );

        let resized = tree
            .compute(root, LayoutSize::new(480.0, 300.0), &mut zero_measure)
            .unwrap();
        assert_eq!(
            resized.get(root).unwrap().border_box,
            LayoutRect {
                x: 0.0,
                y: 0.0,
                width: 480.0,
                height: 300.0,
            }
        );
    }

    #[test]
    fn surface_root_is_the_containing_block_for_absolute_application_root() {
        let root = id(1);
        let mut tree = LayoutTree::new();
        let mut style = ComputedLayoutStyle {
            position: PositionValue::Absolute,
            size: Axes {
                width: ComputedSizeValue::Value(ComputedLengthPercentage::new(0.0, 0.5)),
                height: ComputedSizeValue::Value(ComputedLengthPercentage::new(0.0, 0.25)),
            },
            ..ComputedLayoutStyle::default()
        };
        style.inset.left =
            ComputedLengthPercentageAuto::Value(ComputedLengthPercentage::new(10.0, 0.0));
        style.inset.top =
            ComputedLengthPercentageAuto::Value(ComputedLengthPercentage::new(20.0, 0.0));
        tree.create_node(root, style).unwrap();

        let snapshot = tree
            .compute(root, LayoutSize::new(320.0, 240.0), &mut zero_measure)
            .unwrap();
        assert_eq!(
            snapshot.get(root).unwrap().border_box,
            LayoutRect {
                x: 10.0,
                y: 20.0,
                width: 160.0,
                height: 60.0,
            }
        );
    }

    #[test]
    fn surface_root_can_switch_and_reuse_application_node_ids() {
        let first = id(1);
        let second = id(2);
        let mut tree = LayoutTree::new();
        tree.create_node(first, sized(10.0, 10.0)).unwrap();
        tree.create_node(second, sized(20.0, 20.0)).unwrap();

        tree.compute(first, LayoutSize::new(100.0, 100.0), &mut zero_measure)
            .unwrap();
        let second_snapshot = tree
            .compute(second, LayoutSize::new(100.0, 100.0), &mut zero_measure)
            .unwrap();
        assert_eq!(second_snapshot.len(), 1);
        assert_eq!(second_snapshot.get(second).unwrap().border_box.width, 20.0);

        tree.remove_subtree(second).unwrap();
        tree.create_node(second, sized(30.0, 30.0)).unwrap();
        let reused = tree
            .compute(second, LayoutSize::new(100.0, 100.0), &mut zero_measure)
            .unwrap();
        assert_eq!(reused.get(second).unwrap().border_box.width, 30.0);
    }

    #[test]
    fn mutations_enforce_tree_invariants_and_remove_subtrees() {
        let root = id(1);
        let child = id(2);
        let grandchild = id(3);
        let other = id(4);
        let unknown = id(99);
        let mut tree = LayoutTree::new();
        for node in [root, child, grandchild, other] {
            tree.create_node(node, ComputedLayoutStyle::default())
                .unwrap();
        }
        assert_eq!(
            tree.create_node(root, ComputedLayoutStyle::default()),
            Err(LayoutError::DuplicateNode(root))
        );
        assert_eq!(
            tree.set_children(unknown, &[]),
            Err(LayoutError::UnknownNode(unknown))
        );
        assert_eq!(
            tree.set_children(root, &[unknown]),
            Err(LayoutError::UnknownNode(unknown))
        );
        assert_eq!(
            tree.set_children(root, &[child, child]),
            Err(LayoutError::DuplicateChild {
                parent: root,
                child
            })
        );

        tree.set_children(root, &[child]).unwrap();
        tree.set_children(root, &[child]).unwrap();
        tree.set_children(root, &[]).unwrap();
        tree.set_children(root, &[child]).unwrap();
        tree.set_children(child, &[grandchild]).unwrap();
        assert_eq!(
            tree.set_children(child, &[child]),
            Err(LayoutError::TreeCycle {
                parent: child,
                child
            })
        );
        assert_eq!(
            tree.set_children(child, &[root]),
            Err(LayoutError::TreeCycle {
                parent: child,
                child: root
            })
        );
        assert_eq!(
            tree.set_children(other, &[grandchild]),
            Err(LayoutError::ChildAlreadyAttached {
                child: grandchild,
                parent: child
            })
        );
        assert_eq!(
            tree.compute(child, LayoutSize::new(10.0, 10.0), &mut zero_measure),
            Err(LayoutError::RootHasParent {
                root: child,
                parent: root
            })
        );

        tree.remove_subtree(child).unwrap();
        assert_eq!(tree.len(), 2);
        assert!(!tree.contains(child));
        assert!(!tree.contains(grandchild));
        assert_eq!(
            tree.remove_subtree(unknown),
            Err(LayoutError::UnknownNode(unknown))
        );
        tree.remove_subtree(root).unwrap();
        tree.remove_subtree(other).unwrap();
        assert!(tree.is_empty());
    }

    #[test]
    fn mutation_unknowns_and_viewport_validation_are_reported() {
        let unknown = id(8);
        let mut tree = LayoutTree::new();
        assert_eq!(
            tree.update_style(unknown, ComputedLayoutStyle::default()),
            Err(LayoutError::UnknownNode(unknown))
        );
        assert_eq!(
            tree.set_measurable(unknown, true),
            Err(LayoutError::UnknownNode(unknown))
        );
        assert_eq!(
            tree.invalidate_measurement(unknown),
            Err(LayoutError::UnknownNode(unknown))
        );
        assert_eq!(
            tree.compute(unknown, LayoutSize::new(1.0, 1.0), &mut zero_measure),
            Err(LayoutError::UnknownNode(unknown))
        );

        let root = id(1);
        tree.create_node(root, ComputedLayoutStyle::default())
            .unwrap();
        for viewport in [
            LayoutSize::new(-1.0, 1.0),
            LayoutSize::new(1.0, -1.0),
            LayoutSize::new(f32::INFINITY, 1.0),
            LayoutSize::new(1.0, f32::NAN),
        ] {
            assert_eq!(
                tree.compute(root, viewport, &mut zero_measure),
                Err(LayoutError::InvalidViewport)
            );
        }
        assert_eq!(LayoutSize::default(), LayoutSize::new(0.0, 0.0));
        assert_eq!(
            format!("{}", LayoutError::InvalidViewport),
            "InvalidViewport"
        );
        let error: &dyn Error = &LayoutError::InvalidViewport;
        assert!(error.source().is_none());
    }

    #[test]
    fn reparent_moves_attached_and_unattached_nodes_atomically() {
        let first_parent = id(1);
        let second_parent = id(2);
        let first = id(3);
        let moved = id(4);
        let unattached = id(5);
        let unknown = id(99);
        let mut tree = LayoutTree::new();
        for node in [first_parent, second_parent, first, moved, unattached] {
            tree.create_node(node, ComputedLayoutStyle::default())
                .unwrap();
        }
        tree.set_children(first_parent, &[first, moved]).unwrap();

        tree.reparent(moved, first_parent, 0).unwrap();
        assert_eq!(tree.nodes[&first_parent].children, [moved, first]);
        tree.reparent(moved, second_parent, 0).unwrap();
        assert_eq!(tree.nodes[&first_parent].children, [first]);
        assert_eq!(tree.nodes[&second_parent].children, [moved]);
        tree.reparent(unattached, second_parent, 1).unwrap();
        assert_eq!(tree.nodes[&second_parent].children, [moved, unattached]);

        assert_eq!(
            tree.reparent(unknown, second_parent, 0),
            Err(LayoutError::UnknownNode(unknown))
        );
        assert_eq!(
            tree.reparent(first, unknown, 0),
            Err(LayoutError::UnknownNode(unknown))
        );
        assert_eq!(
            tree.reparent(first_parent, first, 0),
            Err(LayoutError::TreeCycle {
                parent: first,
                child: first_parent
            })
        );
        assert_eq!(
            tree.reparent(second_parent, second_parent, 0),
            Err(LayoutError::TreeCycle {
                parent: second_parent,
                child: second_parent
            })
        );
        assert_eq!(
            tree.reparent(first, first_parent, 2),
            Err(LayoutError::ChildIndexOutOfBounds {
                parent: first_parent,
                index: 2,
                max_index: 0
            })
        );
        assert_eq!(tree.nodes[&first_parent].children, [first]);
    }

    #[test]
    fn updates_report_impacts_and_resort_siblings() {
        let root = id(1);
        let left = id(2);
        let right = id(3);
        let mut tree = LayoutTree::new();
        tree.create_node(root, sized(30.0, 10.0)).unwrap();
        tree.create_node(left, sized(10.0, 10.0)).unwrap();
        tree.create_node(right, sized(10.0, 10.0)).unwrap();
        assert_eq!(LayoutTree::validate_style(&sized(1.0, 1.0)), Ok(()));
        tree.set_children(root, &[left, right]).unwrap();
        let mut resized_root = sized(31.0, 10.0);
        resized_root.display = DisplayValue::Linear;
        assert!(
            tree.update_style(root, resized_root)
                .unwrap()
                .contains(PropertyImpactSet::LAYOUT)
        );
        assert!(
            tree.update_style(left, sized(10.0, 10.0))
                .unwrap()
                .is_empty()
        );

        let mut reordered = sized(10.0, 10.0);
        reordered.order = -1;
        assert!(
            tree.update_style(right, reordered)
                .unwrap()
                .contains(PropertyImpactSet::LAYOUT)
        );
        let snapshot = tree
            .compute(root, LayoutSize::new(30.0, 10.0), &mut zero_measure)
            .unwrap();
        assert_eq!(snapshot.get(right).unwrap().border_box.x, 0.0);
        assert_eq!(snapshot.get(left).unwrap().border_box.x, 10.0);

        let mut unsupported = sized(10.0, 10.0);
        unsupported.position = PositionValue::Fixed;
        assert_eq!(
            tree.update_style(left, unsupported),
            Err(LayoutError::UnsupportedStyle(
                UnsupportedLayoutFeature::FixedPosition
            ))
        );
        let mut unsupported = sized(10.0, 10.0);
        unsupported.position = PositionValue::Sticky;
        assert_eq!(
            LayoutTree::validate_style(&unsupported),
            Err(LayoutError::UnsupportedStyle(
                UnsupportedLayoutFeature::StickyPosition
            ))
        );
        assert_eq!(snapshot.get(id(44)), None);
    }

    #[test]
    fn all_supported_style_spellings_convert() {
        let mut style = ComputedLayoutStyle {
            display: DisplayValue::None,
            position: PositionValue::Absolute,
            direction: DirectionValue::Rtl,
            box_sizing: BoxSizingValue::ContentBox,
            size: Axes {
                width: ComputedSizeValue::Value(ComputedLengthPercentage::new(0.0, 0.5)),
                height: ComputedSizeValue::Auto,
            },
            min_size: Axes {
                width: ComputedSizeValue::None,
                height: ComputedSizeValue::Value(ComputedLengthPercentage::new(2.0, 0.0)),
            },
            max_size: Axes {
                width: ComputedSizeValue::None,
                height: ComputedSizeValue::None,
            },
            margin: Edges {
                top: ComputedLengthPercentageAuto::Auto,
                right: ComputedLengthPercentageAuto::Value(ComputedLengthPercentage::new(0.0, 0.1)),
                bottom: ComputedLengthPercentageAuto::Value(ComputedLengthPercentage::new(
                    2.0, 0.0,
                )),
                left: ComputedLengthPercentageAuto::Auto,
            },
            padding: Edges {
                top: ComputedLengthPercentage::new(-2.0, 0.0),
                right: ComputedLengthPercentage::new(0.0, -0.2),
                bottom: ComputedLengthPercentage::ZERO,
                left: ComputedLengthPercentage::ZERO,
            },
            border: Edges {
                top: ComputedLengthPercentage::new(1.0, 0.0),
                right: ComputedLengthPercentage::new(2.0, 0.0),
                bottom: ComputedLengthPercentage::new(3.0, 0.0),
                left: ComputedLengthPercentage::new(4.0, 0.0),
            },
            inset: Edges {
                top: ComputedLengthPercentageAuto::Auto,
                right: ComputedLengthPercentageAuto::Auto,
                bottom: ComputedLengthPercentageAuto::Auto,
                left: ComputedLengthPercentageAuto::Auto,
            },
            flex_direction: FlexDirectionValue::RowReverse,
            flex_wrap: FlexWrapValue::WrapReverse,
            flex_grow: StyleNumber::new(2.0),
            flex_shrink: StyleNumber::new(3.0),
            flex_basis: ComputedFlexBasis::Value(ComputedLengthPercentage::new(3.0, 0.0)),
            justify_content: JustifyContentValue::Start,
            align_items: AlignItemsValue::Start,
            align_self: AlignSelfValue::Auto,
            align_content: AlignContentValue::SpaceEvenly,
            gap: Axes {
                width: ComputedLengthPercentage::new(-1.0, 0.0),
                height: ComputedLengthPercentage::new(0.0, -0.1),
            },
            aspect_ratio: Some(StyleNumber::new(1.5)),
            order: 7,
            ..ComputedLayoutStyle::default()
        };
        let converted = convert_style(&style).unwrap();
        assert_eq!(converted.display, Display::None);
        assert_eq!(converted.position, Position::Absolute);
        assert_eq!(converted.direction, Direction::Rtl);
        assert_eq!(converted.box_sizing, BoxSizing::ContentBox);
        assert_eq!(converted.size.width.value(), 0.5);
        assert_eq!(converted.aspect_ratio, Some(1.5));

        for display in [
            DisplayValue::Flex,
            DisplayValue::Grid,
            DisplayValue::Linear,
            DisplayValue::Relative,
        ] {
            style.display = display;
            convert_style(&style).unwrap();
        }
        for direction in [
            FlexDirectionValue::Row,
            FlexDirectionValue::Column,
            FlexDirectionValue::ColumnReverse,
        ] {
            style.flex_direction = direction;
            convert_style(&style).unwrap();
        }
        for wrap in [FlexWrapValue::NoWrap, FlexWrapValue::Wrap] {
            style.flex_wrap = wrap;
            convert_style(&style).unwrap();
        }
        for value in [
            AlignItemsValue::Stretch,
            AlignItemsValue::FlexStart,
            AlignItemsValue::FlexEnd,
            AlignItemsValue::Center,
            AlignItemsValue::Baseline,
            AlignItemsValue::End,
        ] {
            style.align_items = value;
            convert_style(&style).unwrap();
        }
        for value in [
            AlignSelfValue::Stretch,
            AlignSelfValue::FlexStart,
            AlignSelfValue::FlexEnd,
            AlignSelfValue::Center,
            AlignSelfValue::Baseline,
            AlignSelfValue::Start,
            AlignSelfValue::End,
        ] {
            style.align_self = value;
            convert_style(&style).unwrap();
        }
        for value in [
            AlignContentValue::Stretch,
            AlignContentValue::FlexStart,
            AlignContentValue::FlexEnd,
            AlignContentValue::Center,
            AlignContentValue::SpaceBetween,
            AlignContentValue::SpaceAround,
        ] {
            style.align_content = value;
            convert_style(&style).unwrap();
        }
        for value in [
            JustifyContentValue::Stretch,
            JustifyContentValue::FlexStart,
            JustifyContentValue::FlexEnd,
            JustifyContentValue::Center,
            JustifyContentValue::SpaceBetween,
            JustifyContentValue::SpaceAround,
            JustifyContentValue::SpaceEvenly,
            JustifyContentValue::End,
        ] {
            style.justify_content = value;
            convert_style(&style).unwrap();
        }
        style.position = PositionValue::Relative;
        style.direction = DirectionValue::Ltr;
        style.box_sizing = BoxSizingValue::BorderBox;
        style.flex_basis = ComputedFlexBasis::Auto;
        style.aspect_ratio = None;
        convert_style(&style).unwrap();
    }

    #[test]
    fn unsupported_style_features_are_never_silently_lowered() {
        let mut style = ComputedLayoutStyle {
            position: PositionValue::Fixed,
            ..ComputedLayoutStyle::default()
        };
        assert_unsupported(style.clone(), UnsupportedLayoutFeature::FixedPosition);
        style.position = PositionValue::Sticky;
        assert_unsupported(style.clone(), UnsupportedLayoutFeature::StickyPosition);
        style.position = PositionValue::Relative;

        style.size.width = ComputedSizeValue::MaxContent;
        assert_unsupported(style.clone(), UnsupportedLayoutFeature::MaxContent);
        style.size.width = ComputedSizeValue::MinContent;
        assert_unsupported(style.clone(), UnsupportedLayoutFeature::MinContent);
        style.size.width = ComputedSizeValue::FitContent(None);
        assert_unsupported(style.clone(), UnsupportedLayoutFeature::FitContent);
        style.size.width = ComputedSizeValue::Auto;

        style.flex_basis = ComputedFlexBasis::Content;
        assert_unsupported(style.clone(), UnsupportedLayoutFeature::ContentFlexBasis);
        style.flex_basis = ComputedFlexBasis::Value(MIXED);
        assert_unsupported(
            style.clone(),
            UnsupportedLayoutFeature::MixedLengthPercentage,
        );
        style.flex_basis = ComputedFlexBasis::Auto;
        style.gap.width = ComputedLengthPercentage::new(1.0, 0.5);
        assert_unsupported(style, UnsupportedLayoutFeature::MixedLengthPercentage);

        let node = id(1);
        let mut tree = LayoutTree::new();
        let unsupported = ComputedLayoutStyle {
            position: PositionValue::Fixed,
            ..ComputedLayoutStyle::default()
        };
        assert_eq!(
            tree.create_node(node, unsupported),
            Err(LayoutError::UnsupportedStyle(
                UnsupportedLayoutFeature::FixedPosition
            ))
        );
        assert!(!tree.contains(node));
    }

    #[test]
    fn every_style_field_propagates_mixed_value_rejection() {
        let size_setters: [fn(&mut ComputedLayoutStyle); 5] = [
            |style| style.size.height = ComputedSizeValue::Value(MIXED),
            |style| style.min_size.width = ComputedSizeValue::Value(MIXED),
            |style| style.min_size.height = ComputedSizeValue::Value(MIXED),
            |style| style.max_size.width = ComputedSizeValue::Value(MIXED),
            |style| style.max_size.height = ComputedSizeValue::Value(MIXED),
        ];
        let auto_setters: [fn(&mut ComputedLayoutStyle); 8] = [
            |style| style.margin.top = ComputedLengthPercentageAuto::Value(MIXED),
            |style| style.margin.right = ComputedLengthPercentageAuto::Value(MIXED),
            |style| style.margin.bottom = ComputedLengthPercentageAuto::Value(MIXED),
            |style| style.margin.left = ComputedLengthPercentageAuto::Value(MIXED),
            |style| style.inset.top = ComputedLengthPercentageAuto::Value(MIXED),
            |style| style.inset.right = ComputedLengthPercentageAuto::Value(MIXED),
            |style| style.inset.bottom = ComputedLengthPercentageAuto::Value(MIXED),
            |style| style.inset.left = ComputedLengthPercentageAuto::Value(MIXED),
        ];
        let length_setters: [fn(&mut ComputedLayoutStyle); 9] = [
            |style| style.padding.top = MIXED,
            |style| style.padding.right = MIXED,
            |style| style.padding.bottom = MIXED,
            |style| style.padding.left = MIXED,
            |style| style.border.top = MIXED,
            |style| style.border.right = MIXED,
            |style| style.border.bottom = MIXED,
            |style| style.border.left = MIXED,
            |style| style.gap.height = MIXED,
        ];
        for setter in size_setters
            .into_iter()
            .chain(auto_setters)
            .chain(length_setters)
        {
            let mut style = ComputedLayoutStyle::default();
            setter(&mut style);
            assert_unsupported(style, UnsupportedLayoutFeature::MixedLengthPercentage);
        }
    }

    #[test]
    fn available_space_translation_covers_intrinsic_constraints() {
        let request = MeasureRequest {
            known_dimensions: [None, None],
            available_space: [AvailableSpace::MaxContent, AvailableSpace::MinContent],
        };
        let mut measure = zero_measure;
        assert_eq!(
            IntrinsicMeasurer::measure(&mut measure, id(1), request),
            LayoutSize::default()
        );
        assert_eq!(
            from_taffy_available(TaffyAvailableSpace::Definite(3.0)),
            AvailableSpace::Definite(3.0)
        );
        assert_eq!(
            from_taffy_available(TaffyAvailableSpace::MinContent),
            AvailableSpace::MinContent
        );
        assert_eq!(
            from_taffy_available(TaffyAvailableSpace::MaxContent),
            AvailableSpace::MaxContent
        );
    }
}
