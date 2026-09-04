//! Rust-owned windowing for the built-in `list` control primitive.
//!
//! A list is deliberately not a Host element. The authoring builder creates
//! the ordinary built-in `ScrollView`; this module keeps only a bounded set of
//! ordinary item subtrees mounted below it and uses two presentation-only
//! spacer Views to preserve the complete scroll extent. Hosts therefore need
//! no list-specific ABI, view class, recycling contract, or data source.

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::rc::Rc;

use whisker_engine::whisker_layout::LayoutParticipation;
use whisker_style::{
    GridPlacementValue, LengthPercentageAutoValue, LengthPercentageValue, LengthUnit, LengthValue,
    PositionValue, SizeValue, SpecifiedStyle, StyleNumber, StyleProperty, StyleValue,
};
use whisker_value::WhiskerValue;

use crate::element::ElementTag;
use crate::reactive::{Owner, ReadSignal, RwSignal, effect, on_cleanup};

use super::handle::Element;
use super::list::{ListRef, ListScrollTarget, ScrollAlignment, ScrollAxis};
use super::renderer::{
    BindType, append_child, children_of, create_element, insert_child_at, observe_layout,
    observe_layout_batch_end, remove_child, set_event_listener, set_specified_style,
    specified_style, try_invoke_element_command,
};

const DEFAULT_ITEM_SIZE: f32 = 44.0;
const DEFAULT_VIEWPORT_SIZE: f32 = 600.0;
const DEFAULT_OVERSCAN_ITEMS: usize = 2;
const DEFAULT_OVERSCAN_VIEWPORTS: f32 = 1.0;

/// Internal layout policy selected from `list(content_style: ...)`.
///
/// This is renderer-independent metadata, not a Host List contract. The
/// ordinary linear policy keeps one item in each virtual track. The Grid
/// policy groups a fixed number of source-order items into a Taffy Grid row
/// (or column for a horizontal List).
#[doc(hidden)]
#[derive(Clone)]
pub enum VirtualListLayout {
    Linear,
    Grid(VirtualGridLayout),
}

/// Taffy style and grouping information for the supported virtual Grid
/// subset. Unsupported Grid syntax is rejected by the authoring layer before
/// this reaches the virtualizer.
#[doc(hidden)]
#[derive(Clone)]
pub struct VirtualGridLayout {
    pub items_per_track: usize,
    pub track_style: SpecifiedStyle,
    pub cell_style: SpecifiedStyle,
    pub main_gap: f32,
}

/// Internal behavior and auxiliary content for one virtualized List.
#[doc(hidden)]
pub struct VirtualListOptions<K: 'static> {
    pub axis: ScrollAxis,
    pub layout: VirtualListLayout,
    pub list_ref: Option<ListRef<K>>,
    pub initial_scroll: Option<ListScrollTarget<K>>,
    pub start_reached_threshold: f32,
    pub end_reached_threshold: f32,
    pub on_start_reached: Option<Rc<dyn Fn()>>,
    pub on_end_reached: Option<Rc<dyn Fn()>>,
    pub header: Option<Element>,
    pub footer: Option<Element>,
    pub empty: Option<Element>,
}

type ReconcileCallback = Rc<dyn Fn()>;
type EntryLayoutObserver<K> = Rc<dyn Fn(&K, Element)>;

struct PendingEntryMeasurement<K> {
    key: K,
    handle: Element,
    size: f32,
}

#[derive(Clone, Copy)]
struct PendingTrackMeasurement {
    track: usize,
    handle: Element,
    size: f32,
}

#[derive(Clone, Copy)]
struct PendingAuxMeasurement {
    content: AuxContent,
    size: f32,
}

struct MountedEntry<T: 'static> {
    owner: Owner,
    handle: Element,
    mount_handle: Element,
    item: RwSignal<T>,
}

struct MountedTrack<K> {
    owner: Owner,
    handle: Element,
    keys: Vec<K>,
}

struct LayoutIndex<T, K> {
    items: Vec<T>,
    keys: Vec<K>,
    /// Stable-key lookup kept in sync with `keys`. Hot measurement, anchor,
    /// and imperative-scroll paths must not linearly scan large data sets.
    key_to_index: HashMap<K, usize>,
    /// Prefix offsets. `offsets[i]` is the start of item `i` and the final
    /// entry is the complete estimated extent.
    offsets: Vec<f32>,
    sizes: Vec<f32>,
    /// Measured main-axis content size keyed by the first item in a virtual
    /// track. Grid gaps are added separately from the measured content.
    measured_sizes: HashMap<K, f32>,
    /// Automatically learned fallback for tracks that have not been mounted
    /// yet. A developer-provided estimate is deliberately not part of the
    /// public List API.
    estimated_track_size: f32,
    header_extent: f32,
    footer_extent: f32,
    empty_extent: f32,
    items_per_track: usize,
    main_gap: f32,
    generation: u64,
    source_generation: u64,
}

impl<T, K> LayoutIndex<T, K>
where
    K: Eq + Hash + Clone,
{
    fn new(items_per_track: usize, main_gap: f32) -> Self {
        Self {
            items: Vec::new(),
            keys: Vec::new(),
            key_to_index: HashMap::new(),
            offsets: vec![0.0],
            sizes: Vec::new(),
            measured_sizes: HashMap::new(),
            estimated_track_size: DEFAULT_ITEM_SIZE,
            header_extent: 0.0,
            footer_extent: 0.0,
            empty_extent: 0.0,
            items_per_track: items_per_track.max(1),
            main_gap: main_gap.max(0.0),
            generation: 0,
            source_generation: 0,
        }
    }

    fn replace(&mut self, items: Vec<T>, key: impl Fn(&T) -> K) {
        let mut unique_keys = HashSet::with_capacity(items.len());
        self.keys.clear();
        self.keys.reserve(items.len());
        self.key_to_index.clear();
        self.key_to_index.reserve(items.len());
        for (index, item) in items.iter().enumerate() {
            let key = key(item);
            assert!(
                unique_keys.insert(key.clone()),
                "virtualized List keys must be unique"
            );
            self.key_to_index.insert(key.clone(), index);
            self.keys.push(key);
        }
        self.measured_sizes
            .retain(|key, _| unique_keys.contains(key));
        self.refresh_estimated_track_size();
        self.rebuild_sizes_and_offsets();
        self.items = items;
        self.generation = self.generation.wrapping_add(1);
        self.source_generation = self.source_generation.wrapping_add(1);
    }

    fn update_measurements(&mut self, measurements: impl IntoIterator<Item = (K, f32)>) -> bool {
        let mut cache_changed = false;
        for (key, size) in measurements {
            if !size.is_finite() || size < 0.0 {
                continue;
            }
            let Some(&item_index) = self.key_to_index.get(&key) else {
                continue;
            };
            let index = self.track_start_item(self.track_for_item(item_index));
            let track_key = self.keys[index].clone();
            if self
                .measured_sizes
                .get(&track_key)
                .is_some_and(|measured| (*measured - size).abs() < 0.5)
            {
                continue;
            }
            self.measured_sizes.insert(track_key, size);
            cache_changed = true;
        }
        if !cache_changed {
            return false;
        }

        self.refresh_estimated_track_size();
        let mut first_changed = None::<usize>;
        for index in (0..self.keys.len()).step_by(self.items_per_track) {
            let size = self.track_extent(index);
            if (self.sizes[index] - size).abs() < 0.5 {
                continue;
            }
            self.sizes[index] = size;
            first_changed = Some(first_changed.map_or(index, |first| first.min(index)));
        }
        let Some(first_changed) = first_changed else {
            return false;
        };
        for offset_index in first_changed + 1..self.offsets.len() {
            self.offsets[offset_index] =
                self.offsets[offset_index - 1] + self.sizes[offset_index - 1];
        }
        self.generation = self.generation.wrapping_add(1);
        true
    }

    fn refresh_estimated_track_size(&mut self) {
        let mut total = 0.0_f64;
        let mut samples = 0_usize;
        for (key, size) in &self.measured_sizes {
            let Some(index) = self.key_to_index.get(key).copied() else {
                continue;
            };
            if index % self.items_per_track != 0 || *size <= 0.0 {
                continue;
            }
            total += f64::from(*size);
            samples += 1;
        }
        if samples > 0 {
            self.estimated_track_size = (total / samples as f64) as f32;
        }
    }

    fn rebuild_sizes_and_offsets(&mut self) {
        self.offsets.clear();
        self.offsets.reserve(self.keys.len() + 1);
        self.offsets.push(self.header_extent);
        self.sizes.clear();
        self.sizes.reserve(self.keys.len());
        for index in 0..self.keys.len() {
            let size = if index % self.items_per_track == 0 {
                self.track_extent(index)
            } else {
                0.0
            };
            self.sizes.push(size);
            self.offsets
                .push(self.offsets.last().copied().unwrap_or(self.header_extent) + size);
        }
    }

    fn track_extent(&self, index: usize) -> f32 {
        let content_size = self
            .measured_sizes
            .get(&self.keys[index])
            .copied()
            .unwrap_or(self.estimated_track_size);
        let has_successor = index + self.items_per_track < self.keys.len();
        content_size + if has_successor { self.main_gap } else { 0.0 }
    }

    fn update_aux_measurement(&mut self, content: AuxContent, size: f32) -> bool {
        if !size.is_finite() || size < 0.0 {
            return false;
        }
        let current = match content {
            AuxContent::Header => &mut self.header_extent,
            AuxContent::Footer => &mut self.footer_extent,
            AuxContent::Empty => &mut self.empty_extent,
        };
        if (*current - size).abs() < 0.5 {
            return false;
        }
        let delta = size - *current;
        *current = size;
        if matches!(content, AuxContent::Header) {
            for offset in &mut self.offsets {
                *offset += delta;
            }
        }
        self.generation = self.generation.wrapping_add(1);
        true
    }

    fn total_extent(&self) -> f32 {
        self.offsets.last().copied().unwrap_or(self.header_extent)
            + self.footer_extent
            + if self.items.is_empty() {
                self.empty_extent
            } else {
                0.0
            }
    }

    fn track_count(&self) -> usize {
        self.items.len().div_ceil(self.items_per_track)
    }

    fn track_for_item(&self, item: usize) -> usize {
        item / self.items_per_track
    }

    fn track_start_item(&self, track: usize) -> usize {
        track
            .saturating_mul(self.items_per_track)
            .min(self.items.len())
    }

    fn track_end_item(&self, track: usize) -> usize {
        self.track_start_item(track.saturating_add(1))
    }

    fn track_start(&self, track: usize) -> f32 {
        self.offsets[self.track_start_item(track)]
    }

    fn track_end(&self, track: usize) -> f32 {
        self.offsets[self.track_end_item(track)]
    }

    fn item_start(&self, item: usize) -> Option<f32> {
        (item < self.items.len()).then(|| self.track_start(self.track_for_item(item)))
    }

    fn item_end(&self, item: usize) -> Option<f32> {
        (item < self.items.len()).then(|| self.track_end(self.track_for_item(item)))
    }

    fn item_offsets(&self) -> (Vec<f32>, Vec<f32>) {
        let mut starts = Vec::with_capacity(self.items.len());
        let mut ends = Vec::with_capacity(self.items.len());
        for item in 0..self.items.len() {
            starts.push(self.item_start(item).expect("item is in range"));
            ends.push(self.item_end(item).expect("item is in range"));
        }
        (starts, ends)
    }

    fn first_track_with_end_after(&self, offset: f32) -> usize {
        let mut low = 0;
        let mut high = self.track_count();
        while low < high {
            let middle = low + (high - low) / 2;
            if self.track_end(middle) <= offset {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        low
    }

    fn first_track_with_start_at_or_after(&self, offset: f32) -> usize {
        let mut low = 0;
        let mut high = self.track_count();
        while low < high {
            let middle = low + (high - low) / 2;
            if self.track_start(middle) < offset {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        low
    }

    fn anchor(&self, offset: f32) -> Option<(K, f32)> {
        let track = self
            .first_track_with_end_after(offset)
            .min(self.track_count());
        let index = self.track_start_item(track);
        let key = self.keys.get(index)?.clone();
        Some((key, offset - self.track_start(track)))
    }

    fn anchored_offset(&self, key: &K, within_item: f32) -> Option<f32> {
        let index = *self.key_to_index.get(key)?;
        Some(self.item_start(index)? + within_item)
    }

    fn window(&self, geometry: ScrollGeometry) -> LayoutWindow {
        let track_count = self.track_count();
        let first_visible_track = self
            .first_track_with_end_after(geometry.offset)
            .min(track_count);
        let overscan_extent = geometry.viewport.max(0.0) * DEFAULT_OVERSCAN_VIEWPORTS;
        let overscan_start = (geometry.offset - overscan_extent).max(0.0);
        let first_in_overscan = self
            .first_track_with_end_after(overscan_start)
            .min(track_count);
        let start_track =
            first_in_overscan.min(first_visible_track.saturating_sub(DEFAULT_OVERSCAN_ITEMS));
        let visible_end = geometry.offset + geometry.viewport.max(0.0);
        let first_after_viewport = self
            .first_track_with_start_at_or_after(visible_end)
            .max(first_visible_track.saturating_add(1).min(track_count));
        let total_extent = self.total_extent();
        let overscan_end = (visible_end + overscan_extent).min(total_extent);
        let first_after_overscan = self.first_track_with_start_at_or_after(overscan_end);
        let end_track = first_after_overscan
            .max(first_after_viewport.saturating_add(DEFAULT_OVERSCAN_ITEMS))
            .min(track_count);
        let start = self.track_start_item(start_track);
        let end = self.track_start_item(end_track);

        LayoutWindow {
            generation: self.generation,
            source_generation: self.source_generation,
            start,
            end,
            start_track,
            end_track,
            leading_extent: self.track_start(start_track) - self.header_extent,
            trailing_extent: self.track_start(track_count) - self.track_start(end_track),
        }
    }
}

#[derive(Clone, Copy)]
enum AuxContent {
    Header,
    Footer,
    Empty,
}

#[derive(Clone, Copy)]
struct LayoutWindow {
    generation: u64,
    source_generation: u64,
    start: usize,
    end: usize,
    start_track: usize,
    end_track: usize,
    leading_extent: f32,
    trailing_extent: f32,
}

impl LayoutWindow {
    fn identity(self) -> (u64, usize, usize) {
        (self.generation, self.start, self.end)
    }
}

#[derive(Clone, Copy)]
struct ScrollGeometry {
    offset: f32,
    viewport: f32,
}

impl Default for ScrollGeometry {
    fn default() -> Self {
        Self {
            offset: 0.0,
            viewport: DEFAULT_VIEWPORT_SIZE,
        }
    }
}

/// Installs Rust-side virtualization below an ordinary ScrollView.
///
/// `each`, `key`, and `children` mirror `ForEach`'s source, identity, and
/// item-builder split. Host `scroll` events update the mounted window;
/// reactive source changes run through the same reconciliation path.
pub fn virtualize<T, K>(
    scroll_view: Element,
    content: Element,
    each: impl Fn() -> Vec<T> + 'static,
    key: impl Fn(&T) -> K + 'static,
    children: impl Fn(ReadSignal<T>) -> Element + 'static,
    options: VirtualListOptions<K>,
) where
    T: Clone + 'static,
    K: Eq + Hash + Clone + 'static,
{
    // Reconciliation can be entered from Host layout/scroll callbacks, where
    // there is deliberately no current reactive owner. Keep every row and
    // Grid track under the List's authoring scope so components mounted later
    // still inherit contexts (RouterHandle, themes, providers, etc.).
    let list_owner = Owner::current().expect("List must be mounted inside a reactive owner");
    let VirtualListOptions {
        axis,
        layout: virtual_layout,
        list_ref,
        initial_scroll,
        start_reached_threshold,
        end_reached_threshold,
        on_start_reached,
        on_end_reached,
        header,
        footer,
        empty,
    } = options;
    let scroll_extent = create_element(ElementTag::View);
    let leading_spacer = create_element(ElementTag::View);
    let trailing_spacer = create_element(ElementTag::View);
    append_child(scroll_view, scroll_extent);
    append_child(scroll_view, content);
    if let Some(header) = header {
        append_child(content, header);
    }
    append_child(content, leading_spacer);
    append_child(content, trailing_spacer);
    if let Some(footer) = footer {
        append_child(content, footer);
    }

    let each = Rc::new(each);
    let key = Rc::new(key);
    let children = Rc::new(children);
    let geometry = Rc::new(RefCell::new(ScrollGeometry::default()));
    let items_per_track = match &virtual_layout {
        VirtualListLayout::Linear => 1,
        VirtualListLayout::Grid(grid) => grid.items_per_track,
    };
    let main_gap = match &virtual_layout {
        VirtualListLayout::Linear => 0.0,
        VirtualListLayout::Grid(grid) => grid.main_gap,
    };
    let layout = Rc::new(RefCell::new(LayoutIndex::<T, K>::new(
        items_per_track,
        main_gap,
    )));
    let mounted: Rc<RefCell<HashMap<K, MountedEntry<T>>>> = Rc::new(RefCell::new(HashMap::new()));
    let mounted_tracks: Rc<RefCell<HashMap<usize, MountedTrack<K>>>> =
        Rc::new(RefCell::new(HashMap::new()));
    let rendered_window = Rc::new(Cell::new(None::<LayoutWindow>));
    let pending_initial_scroll = Rc::new(RefCell::new(initial_scroll));
    let viewport_ready = Rc::new(Cell::new(false));
    let layout_participation = Rc::new(Cell::new(LayoutParticipation::Participating));
    let presented_scroll_extent = Rc::new(Cell::new(0.0));
    let configured_scroll_extent = Rc::new(Cell::new(f32::NAN));
    let inside_start_threshold = Rc::new(Cell::new(false));
    let inside_end_threshold = Rc::new(Cell::new(false));
    let alive = Rc::new(Cell::new(true));
    let pending_entry_measurements: Rc<RefCell<Vec<PendingEntryMeasurement<K>>>> =
        Rc::new(RefCell::new(Vec::new()));
    let pending_track_measurements: Rc<RefCell<Vec<PendingTrackMeasurement>>> =
        Rc::new(RefCell::new(Vec::new()));
    let pending_aux_measurements: Rc<RefCell<Vec<PendingAuxMeasurement>>> =
        Rc::new(RefCell::new(Vec::new()));
    let pending_layout_notification = Rc::new(Cell::new(false));
    if let Some(list_ref) = &list_ref {
        list_ref.bind(scroll_view);
        list_ref.update_geometry(0.0, DEFAULT_VIEWPORT_SIZE);
    }
    let observe_entry: EntryLayoutObserver<K> = {
        let pending_entry_measurements = Rc::clone(&pending_entry_measurements);
        let pending_layout_notification = Rc::clone(&pending_layout_notification);
        Rc::new(move |key, handle| {
            let key = key.clone();
            let pending_entry_measurements = Rc::clone(&pending_entry_measurements);
            let pending_layout_notification = Rc::clone(&pending_layout_notification);
            observe_layout(
                handle,
                Box::new(move |observation| {
                    let layout_geometry = observation.geometry;
                    let size = match axis {
                        ScrollAxis::Vertical => layout_geometry.border_box.height,
                        ScrollAxis::Horizontal => layout_geometry.border_box.width,
                    };
                    pending_entry_measurements
                        .borrow_mut()
                        .push(PendingEntryMeasurement {
                            key: key.clone(),
                            handle,
                            size,
                        });
                    pending_layout_notification.set(true);
                }),
            );
        })
    };
    let observe_track: Rc<dyn Fn(usize, Element)> = {
        let pending_track_measurements = Rc::clone(&pending_track_measurements);
        let pending_layout_notification = Rc::clone(&pending_layout_notification);
        Rc::new(move |track, handle| {
            let pending_track_measurements = Rc::clone(&pending_track_measurements);
            let pending_layout_notification = Rc::clone(&pending_layout_notification);
            observe_layout(
                handle,
                Box::new(move |observation| {
                    let layout_geometry = observation.geometry;
                    let size = match axis {
                        ScrollAxis::Vertical => layout_geometry.border_box.height,
                        ScrollAxis::Horizontal => layout_geometry.border_box.width,
                    };
                    pending_track_measurements
                        .borrow_mut()
                        .push(PendingTrackMeasurement {
                            track,
                            handle,
                            size,
                        });
                    pending_layout_notification.set(true);
                }),
            );
        })
    };

    let observe_aux: Rc<dyn Fn(AuxContent, Element)> = {
        let pending_aux_measurements = Rc::clone(&pending_aux_measurements);
        let pending_layout_notification = Rc::clone(&pending_layout_notification);
        Rc::new(move |content, handle| {
            let pending_aux_measurements = Rc::clone(&pending_aux_measurements);
            let pending_layout_notification = Rc::clone(&pending_layout_notification);
            observe_layout(
                handle,
                Box::new(move |observation| {
                    let layout_geometry = observation.geometry;
                    let size = match axis {
                        ScrollAxis::Vertical => layout_geometry.border_box.height,
                        ScrollAxis::Horizontal => layout_geometry.border_box.width,
                    };
                    pending_aux_measurements
                        .borrow_mut()
                        .push(PendingAuxMeasurement { content, size });
                    pending_layout_notification.set(true);
                }),
            );
        })
    };
    if let Some(header) = header {
        observe_aux(AuxContent::Header, header);
    }
    if let Some(footer) = footer {
        observe_aux(AuxContent::Footer, footer);
    }
    if let Some(empty) = empty {
        observe_aux(AuxContent::Empty, empty);
    }

    let reconcile: ReconcileCallback = {
        let children = Rc::clone(&children);
        let geometry = Rc::clone(&geometry);
        let layout = Rc::clone(&layout);
        let mounted = Rc::clone(&mounted);
        let mounted_tracks = Rc::clone(&mounted_tracks);
        let rendered_window = Rc::clone(&rendered_window);
        let observe_entry = Rc::clone(&observe_entry);
        let observe_track = Rc::clone(&observe_track);
        let virtual_layout = virtual_layout.clone();
        let configured_scroll_extent = Rc::clone(&configured_scroll_extent);
        Rc::new(move || {
            let geometry = *geometry.borrow();
            let window = {
                let layout = layout.borrow();
                let total_extent = layout.total_extent();
                let configured_extent = configured_scroll_extent.get();
                if !configured_extent.is_finite() || (configured_extent - total_extent).abs() >= 0.5
                {
                    configured_scroll_extent.set(total_extent);
                    set_scroll_extent_size(scroll_extent, total_extent, axis);
                }
                let window = layout.window(geometry);
                if rendered_window
                    .get()
                    .is_some_and(|rendered| rendered.identity() == window.identity())
                {
                    return;
                }
                window
            };

            set_spacer_size(leading_spacer, window.leading_extent, axis);
            set_spacer_size(trailing_spacer, window.trailing_extent, axis);

            if let VirtualListLayout::Grid(grid) = &virtual_layout {
                let layout = layout.borrow();
                reconcile_grid_window(
                    content,
                    leading_spacer,
                    trailing_spacer,
                    header,
                    footer,
                    empty,
                    window,
                    &layout,
                    &mut mounted.borrow_mut(),
                    &mut mounted_tracks.borrow_mut(),
                    children.as_ref(),
                    observe_track.as_ref(),
                    grid,
                    axis,
                    list_owner,
                );
                rendered_window.set(Some(window));
                return;
            }

            let previous = rendered_window.get();
            if let Some(previous) =
                previous.filter(|old| old.source_generation == window.source_generation)
            {
                let layout = layout.borrow();
                let mut mounted = mounted.borrow_mut();

                // Within one source generation indices and keys are stable.
                // Visit only the ranges that differ; retained rows require no
                // clone, hash lookup, child-order snapshot, or Host operation.
                visit_range_difference(
                    previous.start,
                    previous.end,
                    window.start,
                    window.end,
                    |index| {
                        let entry = mounted
                            .remove(&layout.keys[index])
                            .expect("a leaving List row must be mounted");
                        remove_child(content, entry.handle);
                        entry.owner.dispose();
                    },
                );
                visit_range_difference(
                    window.start,
                    window.end,
                    previous.start,
                    previous.end,
                    |index| {
                        let key = layout.keys[index].clone();
                        let entry = new_mounted_entry(
                            layout.items[index].clone(),
                            children.as_ref(),
                            None,
                            list_owner,
                        );
                        let handle = entry.handle;
                        observe_entry(&key, handle);
                        let replaced = mounted.insert(key, entry);
                        debug_assert!(replaced.is_none());
                        let prefix = usize::from(header.is_some()) + 1;
                        insert_child_at(content, handle, index - window.start + prefix);
                    },
                );
                rendered_window.set(Some(window));
                return;
            }

            // A source replacement may reorder keys arbitrarily. Preserve keyed
            // entries still in the new window and dispose the rest.
            let mut order = Vec::with_capacity(window.end - window.start + 4);
            if let Some(header) = header {
                order.push(header);
            }
            if layout.borrow().items.is_empty() {
                for (_, entry) in mounted.borrow_mut().drain() {
                    remove_child(content, entry.handle);
                    entry.owner.dispose();
                }
                if let Some(empty) = empty {
                    order.push(empty);
                }
                if let Some(footer) = footer {
                    order.push(footer);
                }
                sync_child_order(content, &order);
                rendered_window.set(Some(window));
                return;
            }
            order.push(leading_spacer);
            {
                let layout = layout.borrow();
                let mut mounted = mounted.borrow_mut();
                let desired_keys = layout.keys[window.start..window.end]
                    .iter()
                    .cloned()
                    .collect::<HashSet<_>>();
                let stale_keys = mounted
                    .keys()
                    .filter(|key| !desired_keys.contains(*key))
                    .cloned()
                    .collect::<Vec<_>>();
                for key in stale_keys {
                    if let Some(entry) = mounted.remove(&key) {
                        remove_child(content, entry.handle);
                        entry.owner.dispose();
                    }
                }

                for index in window.start..window.end {
                    let key = &layout.keys[index];
                    if let Some(entry) = mounted.get_mut(key) {
                        entry.item.set(layout.items[index].clone());
                    } else {
                        let entry = new_mounted_entry(
                            layout.items[index].clone(),
                            children.as_ref(),
                            None,
                            list_owner,
                        );
                        observe_entry(key, entry.handle);
                        mounted.insert(key.clone(), entry);
                    }
                    order.push(
                        mounted
                            .get(key)
                            .expect("the desired List row must be mounted")
                            .handle,
                    );
                }
            }

            order.push(trailing_spacer);
            if let Some(footer) = footer {
                order.push(footer);
            }
            sync_child_order(content, &order);
            rendered_window.set(Some(window));
        })
    };
    {
        let presented_scroll_extent = Rc::clone(&presented_scroll_extent);
        let pending_layout_notification = Rc::clone(&pending_layout_notification);
        observe_layout(
            scroll_extent,
            Box::new(move |observation| {
                let layout_geometry = observation.geometry;
                let extent = match axis {
                    ScrollAxis::Vertical => layout_geometry.border_box.height,
                    ScrollAxis::Horizontal => layout_geometry.border_box.width,
                };
                if !extent.is_finite() || extent < 0.0 {
                    return;
                }
                if observation.participation == LayoutParticipation::Participating {
                    presented_scroll_extent.set(extent);
                }
                pending_layout_notification.set(true);
            }),
        );
    }

    {
        let geometry = Rc::clone(&geometry);
        let list_ref = list_ref.clone();
        let viewport_ready = Rc::clone(&viewport_ready);
        let layout_participation = Rc::clone(&layout_participation);
        let pending_layout_notification = Rc::clone(&pending_layout_notification);
        observe_layout(
            scroll_view,
            Box::new(move |observation| {
                let layout_geometry = observation.geometry;
                let viewport = match axis {
                    ScrollAxis::Vertical => layout_geometry.border_box.height,
                    ScrollAxis::Horizontal => layout_geometry.border_box.width,
                };
                if !viewport.is_finite() || viewport < 0.0 {
                    return;
                }
                layout_participation.set(observation.participation);
                pending_layout_notification.set(true);
                if observation.participation == LayoutParticipation::SuppressedByDisplayNone {
                    return;
                }
                viewport_ready.set(true);
                let current_geometry = *geometry.borrow();
                *geometry.borrow_mut() = ScrollGeometry {
                    offset: current_geometry.offset,
                    viewport,
                };
                if let Some(list_ref) = &list_ref {
                    list_ref.update_geometry(current_geometry.offset, viewport);
                }
            }),
        );
    }

    // A layout pass may resize dozens of mounted rows. Collect their
    // callbacks first, then update the index, anchor, ListRef snapshot, and
    // mounted window once. This mirrors FlashList's commit-level layout
    // collection and avoids N suffix rebuilds plus N reconciliations.
    {
        let geometry = Rc::clone(&geometry);
        let layout = Rc::clone(&layout);
        let mounted = Rc::clone(&mounted);
        let mounted_tracks = Rc::clone(&mounted_tracks);
        let pending_entry_measurements = Rc::clone(&pending_entry_measurements);
        let pending_track_measurements = Rc::clone(&pending_track_measurements);
        let pending_aux_measurements = Rc::clone(&pending_aux_measurements);
        let pending_layout_notification = Rc::clone(&pending_layout_notification);
        let pending_initial_scroll = Rc::clone(&pending_initial_scroll);
        let viewport_ready = Rc::clone(&viewport_ready);
        let layout_participation = Rc::clone(&layout_participation);
        let presented_scroll_extent = Rc::clone(&presented_scroll_extent);
        let alive = Rc::clone(&alive);
        let reconcile = Rc::clone(&reconcile);
        let list_ref = list_ref.clone();
        observe_layout_batch_end(
            scroll_view,
            Box::new(move || {
                if !alive.get() {
                    return;
                }
                if !pending_layout_notification.replace(false) {
                    return;
                }

                // An explicit `display:none` on the List or an ancestor makes
                // descendant geometries inapplicable. Retain the last active
                // index and scroll anchor until the List participates again.
                // A participating zero-sized viewport is intentionally not
                // treated as hidden.
                if layout_participation.get() == LayoutParticipation::SuppressedByDisplayNone {
                    pending_entry_measurements.borrow_mut().clear();
                    pending_track_measurements.borrow_mut().clear();
                    pending_aux_measurements.borrow_mut().clear();
                    return;
                }

                let current_geometry = *geometry.borrow();
                let entry_measurements = pending_entry_measurements
                    .borrow_mut()
                    .drain(..)
                    .filter_map(|pending| {
                        mounted
                            .borrow()
                            .get(&pending.key)
                            .is_some_and(|entry| entry.handle == pending.handle)
                            .then_some((pending.key, pending.size.max(0.0)))
                    })
                    .collect::<Vec<_>>();

                let track_measurements = pending_track_measurements
                    .borrow_mut()
                    .drain(..)
                    .filter_map(|pending| {
                        let tracks = mounted_tracks.borrow();
                        let track = tracks.get(&pending.track)?;
                        (track.handle == pending.handle)
                            .then(|| track.keys.first().cloned())
                            .flatten()
                            .map(|key| (pending.track, key, pending.size.max(0.0)))
                    })
                    .collect::<Vec<_>>();
                let aux_measurements = pending_aux_measurements
                    .borrow_mut()
                    .drain(..)
                    .collect::<Vec<_>>();

                let (changed, corrected_offset) = {
                    let mut layout = layout.borrow_mut();
                    let preserve_item_anchor = !aux_measurements
                        .iter()
                        .any(|pending| matches!(pending.content, AuxContent::Header))
                        || current_geometry.offset >= layout.header_extent;
                    let anchor = preserve_item_anchor
                        .then(|| layout.anchor(current_geometry.offset))
                        .flatten();

                    let mut changed = false;
                    for pending in aux_measurements {
                        changed |=
                            layout.update_aux_measurement(pending.content, pending.size.max(0.0));
                    }
                    let mut measurements = entry_measurements;
                    measurements.extend(
                        track_measurements
                            .into_iter()
                            .map(|(_track, key, size)| (key, size)),
                    );
                    changed |= layout.update_measurements(measurements);

                    let initial_offset = resolve_pending_initial_target(
                        &mut pending_initial_scroll.borrow_mut(),
                        &layout,
                        current_geometry,
                        initial_geometry_ready(
                            viewport_ready.get(),
                            presented_scroll_extent.get(),
                            layout.total_extent(),
                        ),
                    );
                    let corrected_offset = initial_offset.or_else(|| {
                        if changed {
                            anchor.and_then(|(key, within_item)| {
                                layout.anchored_offset(&key, within_item)
                            })
                        } else {
                            None
                        }
                    });
                    if changed && let Some(list_ref) = &list_ref {
                        let (starts, ends) = layout.item_offsets();
                        list_ref.update_layout(&layout.keys, &starts, &ends, layout.total_extent());
                    }
                    (changed, corrected_offset)
                };

                if let Some(offset) = corrected_offset
                    && (offset - current_geometry.offset).abs() >= 0.5
                {
                    geometry.borrow_mut().offset = offset;
                    if let Some(list_ref) = &list_ref {
                        list_ref.update_geometry(offset, current_geometry.viewport);
                    }
                    let _ = try_invoke_element_command(
                        scroll_view,
                        "scrollTo",
                        WhiskerValue::map([
                            ("offset", WhiskerValue::Float(f64::from(offset))),
                            ("smooth", WhiskerValue::Bool(false)),
                        ]),
                    );
                }
                if changed || viewport_ready.get() {
                    reconcile();
                }
            }),
        );
    }

    {
        let each = Rc::clone(&each);
        let key = Rc::clone(&key);
        let layout = Rc::clone(&layout);
        let geometry = Rc::clone(&geometry);
        let reconcile = Rc::clone(&reconcile);
        let list_ref = list_ref.clone();
        let pending_initial_scroll = Rc::clone(&pending_initial_scroll);
        let viewport_ready = Rc::clone(&viewport_ready);
        let presented_scroll_extent = Rc::clone(&presented_scroll_extent);
        effect(move || {
            let items = each();
            let current_geometry = *geometry.borrow();
            let corrected_offset = {
                let mut layout = layout.borrow_mut();
                let anchor = layout.anchor(current_geometry.offset);
                layout.replace(items, |item| key(item));
                let initial_offset = resolve_pending_initial_target(
                    &mut pending_initial_scroll.borrow_mut(),
                    &layout,
                    current_geometry,
                    initial_geometry_ready(
                        viewport_ready.get(),
                        presented_scroll_extent.get(),
                        layout.total_extent(),
                    ),
                );
                initial_offset.or_else(|| {
                    anchor.and_then(|(key, within_item)| layout.anchored_offset(&key, within_item))
                })
            };
            if let Some(offset) = corrected_offset
                && (offset - current_geometry.offset).abs() >= 0.5
            {
                geometry.borrow_mut().offset = offset;
                let _ = try_invoke_element_command(
                    scroll_view,
                    "scrollTo",
                    WhiskerValue::map([
                        ("offset", WhiskerValue::Float(f64::from(offset))),
                        ("smooth", WhiskerValue::Bool(false)),
                    ]),
                );
            }
            if let Some(list_ref) = &list_ref {
                let layout = layout.borrow();
                let (starts, ends) = layout.item_offsets();
                list_ref.update_layout(&layout.keys, &starts, &ends, layout.total_extent());
                list_ref.update_geometry(
                    corrected_offset.unwrap_or(current_geometry.offset),
                    current_geometry.viewport,
                );
            }
            reconcile();
        });
    }

    {
        let geometry = Rc::clone(&geometry);
        let reconcile = Rc::clone(&reconcile);
        let list_ref = list_ref.clone();
        let layout = Rc::clone(&layout);
        let inside_start_threshold = Rc::clone(&inside_start_threshold);
        let inside_end_threshold = Rc::clone(&inside_end_threshold);
        set_event_listener(
            scroll_view,
            "scroll",
            BindType::Bind,
            Box::new(move |event| {
                if let Some(next) = scroll_geometry(&event, axis) {
                    *geometry.borrow_mut() = next;
                    if let Some(list_ref) = &list_ref {
                        list_ref.update_geometry(next.offset, next.viewport);
                    }
                    reconcile();
                    let content_extent = layout.borrow().total_extent();
                    let at_start = next.offset <= start_reached_threshold;
                    let at_end =
                        content_extent - (next.offset + next.viewport) <= end_reached_threshold;
                    let was_at_start = inside_start_threshold.replace(at_start);
                    let was_at_end = inside_end_threshold.replace(at_end);
                    if at_start
                        && !was_at_start
                        && let Some(callback) = &on_start_reached
                    {
                        callback();
                    }
                    if at_end
                        && !was_at_end
                        && let Some(callback) = &on_end_reached
                    {
                        callback();
                    }
                }
            }),
        );
    }

    on_cleanup(move || {
        alive.set(false);
        for (_, track) in mounted_tracks.borrow_mut().drain() {
            for key in &track.keys {
                if let Some(entry) = mounted.borrow().get(key)
                    && children_of(track.handle).contains(&entry.mount_handle)
                {
                    remove_child(track.handle, entry.mount_handle);
                }
            }
            if children_of(content).contains(&track.handle) {
                remove_child(content, track.handle);
            }
            track.owner.dispose();
        }
        for (_, entry) in mounted.borrow_mut().drain() {
            if children_of(content).contains(&entry.mount_handle) {
                remove_child(content, entry.mount_handle);
            }
            entry.owner.dispose();
        }
        let attached = children_of(content);
        if attached.contains(&leading_spacer) {
            remove_child(content, leading_spacer);
        }
        if attached.contains(&trailing_spacer) {
            remove_child(content, trailing_spacer);
        }
        if children_of(scroll_view).contains(&scroll_extent) {
            remove_child(scroll_view, scroll_extent);
        }
        if let Some(header) = header
            && children_of(content).contains(&header)
        {
            remove_child(content, header);
        }
        if let Some(footer) = footer
            && children_of(content).contains(&footer)
        {
            remove_child(content, footer);
        }
        if let Some(empty) = empty
            && children_of(content).contains(&empty)
        {
            remove_child(content, empty);
        }
        remove_child(scroll_view, content);
        if let Some(list_ref) = &list_ref {
            list_ref.unbind();
        }
    });
}

fn resolve_pending_initial_target<T, K: Eq + Hash + Clone>(
    pending: &mut Option<ListScrollTarget<K>>,
    layout: &LayoutIndex<T, K>,
    geometry: ScrollGeometry,
    geometry_ready: bool,
) -> Option<f32> {
    if !geometry_ready {
        return None;
    }
    let target = pending.as_ref()?;
    let offset = resolve_layout_target(layout, target, geometry)?;
    if initial_target_is_measured(layout, target) {
        pending.take();
    }
    Some(offset)
}

fn initial_geometry_ready(
    viewport_ready: bool,
    presented_extent: f32,
    current_extent: f32,
) -> bool {
    viewport_ready && (presented_extent - current_extent).abs() < 0.5
}

fn initial_target_is_measured<T, K: Eq + Hash + Clone>(
    layout: &LayoutIndex<T, K>,
    target: &ListScrollTarget<K>,
) -> bool {
    let target_index = match target {
        ListScrollTarget::Start | ListScrollTarget::Offset(_) => return true,
        ListScrollTarget::End => layout.items.len().checked_sub(1),
        ListScrollTarget::Index { index, .. } => Some(*index),
        ListScrollTarget::Key { key, .. } => layout.key_to_index.get(key).copied(),
    };
    let Some(target_index) = target_index.filter(|index| *index < layout.items.len()) else {
        return false;
    };
    let track_start = layout.track_start_item(layout.track_for_item(target_index));
    layout
        .keys
        .get(track_start)
        .is_some_and(|key| layout.measured_sizes.contains_key(key))
}

fn resolve_layout_target<T, K: Eq + Hash + Clone>(
    layout: &LayoutIndex<T, K>,
    target: &ListScrollTarget<K>,
    geometry: ScrollGeometry,
) -> Option<f32> {
    let content_extent = layout.total_extent();
    let maximum = (content_extent - geometry.viewport).max(0.0);
    let item_offset = |index: usize, alignment: ScrollAlignment| {
        let start = layout.item_start(index)?;
        let end = layout.item_end(index)?;
        let extent = end - start;
        Some(match alignment {
            ScrollAlignment::Start => start,
            ScrollAlignment::Center => start - (geometry.viewport - extent) * 0.5,
            ScrollAlignment::End => end - geometry.viewport,
            ScrollAlignment::Nearest if start < geometry.offset => start,
            ScrollAlignment::Nearest if end > geometry.offset + geometry.viewport => {
                end - geometry.viewport
            }
            ScrollAlignment::Nearest => geometry.offset,
        })
    };
    let offset = match target {
        ListScrollTarget::Start => 0.0,
        ListScrollTarget::End => maximum,
        ListScrollTarget::Offset(offset) => *offset as f32,
        ListScrollTarget::Index { index, alignment } => item_offset(*index, *alignment)?,
        ListScrollTarget::Key { key, alignment } => {
            let index = *layout.key_to_index.get(key)?;
            item_offset(index, *alignment)?
        }
    };
    Some(offset.clamp(0.0, maximum))
}

fn new_mounted_entry<T: Clone + 'static>(
    item: T,
    children: &dyn Fn(ReadSignal<T>) -> Element,
    cell_style: Option<&SpecifiedStyle>,
    parent: Owner,
) -> MountedEntry<T> {
    let owner = Owner::new(Some(parent));
    let (item, handle, mount_handle) = owner.with(|| {
        let item = RwSignal::new(item);
        let handle = children(item.read_only());
        let mount_handle = if let Some(style) = cell_style {
            validate_virtual_grid_item(handle);
            let cell = create_element(ElementTag::View);
            set_specified_style(cell, style);
            append_child(cell, handle);
            cell
        } else {
            handle
        };
        (item, handle, mount_handle)
    });
    MountedEntry {
        owner,
        handle,
        mount_handle,
        item,
    }
}

fn validate_virtual_grid_item(handle: Element) {
    let Some(style) = specified_style(handle) else {
        return;
    };
    for declaration in style.resolved() {
        let property = declaration.property();
        let unsupported_placement = matches!(
            property,
            StyleProperty::GridColumnStart
                | StyleProperty::GridColumnEnd
                | StyleProperty::GridRowStart
                | StyleProperty::GridRowEnd
        ) && !matches!(
            declaration.value(),
            StyleValue::GridPlacement(GridPlacementValue::Auto)
        );
        let unsupported_order = property == StyleProperty::Order
            && !matches!(declaration.value(), StyleValue::Integer(0));
        if unsupported_placement || unsupported_order {
            panic!(
                "unsupported virtualized Grid item: `{}` requires explicit placement",
                property.css_name()
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn reconcile_grid_window<T, K>(
    content: Element,
    leading_spacer: Element,
    trailing_spacer: Element,
    header: Option<Element>,
    footer: Option<Element>,
    empty: Option<Element>,
    window: LayoutWindow,
    layout: &LayoutIndex<T, K>,
    mounted: &mut HashMap<K, MountedEntry<T>>,
    mounted_tracks: &mut HashMap<usize, MountedTrack<K>>,
    children: &dyn Fn(ReadSignal<T>) -> Element,
    observe_track: &dyn Fn(usize, Element),
    grid: &VirtualGridLayout,
    axis: ScrollAxis,
    list_owner: Owner,
) where
    T: Clone + 'static,
    K: Eq + Hash + Clone + 'static,
{
    let mut order = Vec::with_capacity(window.end_track - window.start_track + 4);
    if let Some(header) = header {
        order.push(header);
    }

    if layout.items.is_empty() {
        for (_, track) in mounted_tracks.drain() {
            for key in &track.keys {
                if let Some(entry) = mounted.get(key)
                    && children_of(track.handle).contains(&entry.mount_handle)
                {
                    remove_child(track.handle, entry.mount_handle);
                }
            }
            if children_of(content).contains(&track.handle) {
                remove_child(content, track.handle);
            }
            track.owner.dispose();
        }
        for (_, entry) in mounted.drain() {
            entry.owner.dispose();
        }
        if let Some(empty) = empty {
            order.push(empty);
        }
        if let Some(footer) = footer {
            order.push(footer);
        }
        sync_child_order(content, &order);
        return;
    }

    order.push(leading_spacer);
    let desired_keys = layout.keys[window.start..window.end]
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    let desired_track_keys = (window.start_track..window.end_track)
        .map(|track| {
            let start = layout.track_start_item(track);
            let end = layout.track_end_item(track);
            (track, layout.keys[start..end].to_vec())
        })
        .collect::<HashMap<_, _>>();

    let stale_tracks = mounted_tracks
        .iter()
        .filter_map(|(track, mounted_track)| {
            let keep = desired_track_keys
                .get(track)
                .is_some_and(|keys| keys == &mounted_track.keys);
            (!keep).then_some(*track)
        })
        .collect::<Vec<_>>();
    for track_index in stale_tracks {
        let track = mounted_tracks
            .remove(&track_index)
            .expect("stale Grid track remains mounted");
        for key in &track.keys {
            if let Some(entry) = mounted.get(key)
                && children_of(track.handle).contains(&entry.mount_handle)
            {
                remove_child(track.handle, entry.mount_handle);
            }
        }
        if children_of(content).contains(&track.handle) {
            remove_child(content, track.handle);
        }
        track.owner.dispose();
    }

    let stale_keys = mounted
        .keys()
        .filter(|key| !desired_keys.contains(*key))
        .cloned()
        .collect::<Vec<_>>();
    for key in stale_keys {
        if let Some(entry) = mounted.remove(&key) {
            entry.owner.dispose();
        }
    }

    for track_index in window.start_track..window.end_track {
        let keys = desired_track_keys
            .get(&track_index)
            .expect("desired Grid track keys");
        let track_handle = if let Some(track) = mounted_tracks.get(&track_index) {
            track.handle
        } else {
            let owner = Owner::new(Some(list_owner));
            let handle = owner.with(|| create_element(ElementTag::View));
            mounted_tracks.insert(
                track_index,
                MountedTrack {
                    owner,
                    handle,
                    keys: keys.clone(),
                },
            );
            observe_track(track_index, handle);
            handle
        };
        set_specified_style(
            track_handle,
            &grid_track_style(grid, axis, track_index + 1 < layout.track_count()),
        );

        for item_index in layout.track_start_item(track_index)..layout.track_end_item(track_index) {
            let key = &layout.keys[item_index];
            if let Some(entry) = mounted.get_mut(key) {
                entry.item.set(layout.items[item_index].clone());
            } else {
                mounted.insert(
                    key.clone(),
                    new_mounted_entry(
                        layout.items[item_index].clone(),
                        children,
                        Some(&grid.cell_style),
                        list_owner,
                    ),
                );
            }
            let mount_handle = mounted
                .get(key)
                .expect("desired Grid item remains mounted")
                .mount_handle;
            if !children_of(track_handle).contains(&mount_handle) {
                append_child(track_handle, mount_handle);
            }
        }
        order.push(track_handle);
    }

    order.push(trailing_spacer);
    if let Some(footer) = footer {
        order.push(footer);
    }
    sync_child_order(content, &order);
}

fn grid_track_style(
    grid: &VirtualGridLayout,
    axis: ScrollAxis,
    has_successor: bool,
) -> SpecifiedStyle {
    if !has_successor || grid.main_gap <= 0.0 {
        return grid.track_style.clone();
    }
    let gap = StyleValue::LengthPercentageAuto(LengthPercentageAutoValue::LengthPercentage(
        LengthPercentageValue::Length(LengthValue::Dimension {
            value: StyleNumber::new(grid.main_gap),
            unit: LengthUnit::Px,
        }),
    ));
    let property = match axis {
        ScrollAxis::Vertical => StyleProperty::MarginBottom,
        ScrollAxis::Horizontal => StyleProperty::MarginRight,
    };
    grid.track_style.clone().push(property, gap)
}

/// Visits the indices in `[start, end)` that are not covered by
/// `[other_start, other_end)`, without allocating an intermediate range set.
#[inline]
fn visit_range_difference(
    start: usize,
    end: usize,
    other_start: usize,
    other_end: usize,
    mut visit: impl FnMut(usize),
) {
    let overlap_start = start.max(other_start);
    let overlap_end = end.min(other_end);
    if overlap_start >= overlap_end {
        for index in start..end {
            visit(index);
        }
        return;
    }
    for index in start..overlap_start {
        visit(index);
    }
    for index in overlap_end..end {
        visit(index);
    }
}

/// Makes the current child sequence match `target` while leaving every
/// already-correct child attached. During ordinary scrolling this removes the
/// rows that left one edge and inserts only rows entering the opposite edge.
fn sync_child_order(parent: Element, target: &[Element]) {
    let mut current = children_of(parent);
    for (index, child) in target.iter().copied().enumerate() {
        if current.get(index) == Some(&child) {
            continue;
        }
        if let Some(previous) = current.iter().position(|candidate| *candidate == child) {
            remove_child(parent, child);
            current.remove(previous);
        }
        insert_child_at(parent, child, index);
        current.insert(index, child);
    }
    for child in current.drain(target.len()..) {
        remove_child(parent, child);
    }
}

fn set_spacer_size(element: Element, size: f32, axis: ScrollAxis) {
    let px = LengthPercentageValue::Length(if size == 0.0 {
        LengthValue::Zero
    } else {
        LengthValue::Dimension {
            value: StyleNumber::new(size),
            unit: LengthUnit::Px,
        }
    });
    let size_property = match axis {
        ScrollAxis::Vertical => StyleProperty::Height,
        ScrollAxis::Horizontal => StyleProperty::Width,
    };
    let style = SpecifiedStyle::new()
        .push(
            size_property,
            StyleValue::Size(SizeValue::LengthPercentage(px)),
        )
        .push(
            StyleProperty::FlexShrink,
            StyleValue::Number(StyleNumber::new(0.0)),
        );
    set_specified_style(element, &style);
}

fn set_scroll_extent_size(element: Element, size: f32, axis: ScrollAxis) {
    let px = |value: f32| {
        LengthPercentageValue::Length(if value == 0.0 {
            LengthValue::Zero
        } else {
            LengthValue::Dimension {
                value: StyleNumber::new(value),
                unit: LengthUnit::Px,
            }
        })
    };
    let (width, height) = match axis {
        ScrollAxis::Vertical => (px(1.0), px(size)),
        ScrollAxis::Horizontal => (px(size), px(1.0)),
    };
    let style = SpecifiedStyle::new()
        .push(
            StyleProperty::Position,
            StyleValue::Position(PositionValue::Absolute),
        )
        .push(
            StyleProperty::Width,
            StyleValue::Size(SizeValue::LengthPercentage(width)),
        )
        .push(
            StyleProperty::Height,
            StyleValue::Size(SizeValue::LengthPercentage(height)),
        );
    set_specified_style(element, &style);
}

fn scroll_geometry(event: &WhiskerValue, axis: ScrollAxis) -> Option<ScrollGeometry> {
    let WhiskerValue::Map(event) = event else {
        return None;
    };
    let detail = match event.get("detail") {
        Some(WhiskerValue::Map(detail)) => detail,
        _ => event,
    };
    let number = |name: &str| match detail.get(name) {
        Some(WhiskerValue::Float(value)) => Some(*value as f32),
        Some(WhiskerValue::Int(value)) => Some(*value as f32),
        _ => None,
    };
    let (offset, viewport) = match axis {
        ScrollAxis::Vertical => (number("scrollTop")?, number("viewportHeight")?),
        ScrollAxis::Horizontal => (number("scrollLeft")?, number("viewportWidth")?),
    };
    Some(ScrollGeometry {
        offset: offset.max(0.0),
        viewport: viewport.max(0.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_mounted_from_host_callbacks_inherit_the_list_context() {
        crate::reactive::__reset_for_tests();
        let list_owner = Owner::new(None);
        list_owner.with(|| crate::reactive::provide_context(String::from("router")));

        // Host callbacks enter with no current owner. The explicit List owner
        // must still be the parent of the newly materialized row.
        assert!(Owner::current().is_none());
        let entry = new_mounted_entry(
            1_u32,
            &|_| {
                assert_eq!(
                    crate::reactive::use_context::<String>().as_deref(),
                    Some("router")
                );
                create_element(ElementTag::View)
            },
            None,
            list_owner,
        );

        entry.owner.dispose();
        list_owner.dispose();
    }

    #[test]
    fn visits_only_indices_outside_the_other_range() {
        let collect = |range: (usize, usize), other: (usize, usize)| {
            let mut visited = Vec::new();
            visit_range_difference(range.0, range.1, other.0, other.1, |index| {
                visited.push(index);
            });
            visited
        };

        assert_eq!(collect((3, 10), (5, 8)), vec![3, 4, 8, 9]);
        assert_eq!(collect((3, 6), (8, 10)), vec![3, 4, 5]);
        assert_eq!(collect((3, 6), (0, 10)), Vec::<usize>::new());
    }

    #[test]
    fn reads_wrapped_scroll_geometry() {
        let event = WhiskerValue::map([(
            "detail",
            WhiskerValue::map([
                ("scrollTop", WhiskerValue::Float(120.0)),
                ("viewportHeight", WhiskerValue::Int(480)),
            ]),
        )]);
        let geometry = scroll_geometry(&event, ScrollAxis::Vertical).unwrap();
        assert_eq!(geometry.offset, 120.0);
        assert_eq!(geometry.viewport, 480.0);
    }

    #[test]
    fn reads_horizontal_scroll_geometry() {
        let event = WhiskerValue::map([
            ("scrollLeft", WhiskerValue::Float(240.0)),
            ("viewportWidth", WhiskerValue::Int(320)),
        ]);
        let geometry = scroll_geometry(&event, ScrollAxis::Horizontal).unwrap();
        assert_eq!(geometry.offset, 240.0);
        assert_eq!(geometry.viewport, 320.0);
    }

    #[test]
    fn window_keeps_one_viewport_of_scroll_buffer_on_each_side() {
        let mut index = LayoutIndex::new(1, 0.0);
        index.replace((0_u32..10_000).collect(), |item| *item);

        let geometry = ScrollGeometry {
            offset: 44_000.0,
            viewport: 765.0,
        };
        let window = index.window(geometry);
        let top_buffer = geometry.offset - window.leading_extent;
        let visible_end = geometry.offset + geometry.viewport;
        let rendered_end = index.offsets[window.end];
        let bottom_buffer = rendered_end - visible_end;

        assert!(
            top_buffer >= geometry.viewport,
            "top overscan {top_buffer}px must cover one {viewport}px viewport",
            viewport = geometry.viewport,
        );
        assert!(
            bottom_buffer >= geometry.viewport,
            "bottom overscan {bottom_buffer}px must cover one {viewport}px viewport",
            viewport = geometry.viewport,
        );
    }

    #[test]
    fn key_index_is_rebuilt_after_source_reordering() {
        let mut index = LayoutIndex::new(1, 0.0);
        index.replace(vec!["alpha", "beta", "gamma"], |item| *item);

        assert_eq!(index.key_to_index.get("alpha"), Some(&0));
        assert_eq!(index.key_to_index.get("gamma"), Some(&2));

        index.replace(vec!["gamma", "alpha"], |item| *item);

        assert_eq!(index.key_to_index.get("gamma"), Some(&0));
        assert_eq!(index.key_to_index.get("alpha"), Some(&1));
        assert!(!index.key_to_index.contains_key("beta"));
    }

    #[test]
    fn measurement_batch_rebuilds_the_suffix_and_generation_once() {
        let mut index = LayoutIndex::new(1, 0.0);
        index.replace((0_u32..5).collect(), |item| *item);
        let generation = index.generation;

        assert!(index.update_measurements([(1, 20.0), (3, 80.0)]));

        assert_eq!(index.generation, generation + 1);
        assert_eq!(index.estimated_track_size, 50.0);
        assert_eq!(index.offsets, vec![0.0, 50.0, 70.0, 120.0, 200.0, 250.0]);
        assert_eq!(index.measured_sizes.get(&1), Some(&20.0));
        assert_eq!(index.measured_sizes.get(&3), Some(&80.0));
    }

    #[test]
    fn zero_sized_tracks_do_not_collapse_the_unmeasured_estimate() {
        let mut index = LayoutIndex::new(1, 0.0);
        index.replace((0_u32..3).collect(), |item| *item);

        assert!(index.update_measurements([(0, 0.0)]));
        assert_eq!(index.estimated_track_size, DEFAULT_ITEM_SIZE);
        assert_eq!(index.sizes, vec![0.0, 44.0, 44.0]);

        assert!(index.update_measurements([(1, 100.0)]));
        assert_eq!(index.estimated_track_size, 100.0);
        assert_eq!(index.sizes, vec![0.0, 100.0, 100.0]);
    }

    #[test]
    fn grid_estimates_track_content_and_applies_gap_separately() {
        let mut index = LayoutIndex::new(2, 8.0);
        index.replace((0_u32..6).collect(), |item| *item);
        assert_eq!(index.sizes, vec![52.0, 0.0, 52.0, 0.0, 44.0, 0.0]);

        assert!(index.update_measurements([(0, 100.0), (2, 60.0)]));

        assert_eq!(index.estimated_track_size, 80.0);
        assert_eq!(index.measured_sizes.get(&0), Some(&100.0));
        assert_eq!(index.measured_sizes.get(&2), Some(&60.0));
        assert_eq!(index.sizes, vec![108.0, 0.0, 68.0, 0.0, 80.0, 0.0]);
        assert_eq!(index.total_extent(), 256.0);
    }
}
