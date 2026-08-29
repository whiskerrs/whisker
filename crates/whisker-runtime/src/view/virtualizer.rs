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

use whisker_style::{
    LengthPercentageValue, LengthUnit, LengthValue, SizeValue, SpecifiedStyle, StyleNumber,
    StyleProperty, StyleValue,
};
use whisker_value::WhiskerValue;

use crate::element::ElementTag;
use crate::reactive::{Owner, ReadSignal, RwSignal, effect, on_cleanup};

use super::handle::Element;
use super::renderer::{
    BindType, append_child, children_of, create_element, insert_child_at, remove_child,
    set_event_listener, set_specified_style,
};

const DEFAULT_ITEM_SIZE: f32 = 44.0;
const DEFAULT_VIEWPORT_SIZE: f32 = 600.0;
const DEFAULT_OVERSCAN_ITEMS: usize = 2;
const DEFAULT_OVERSCAN_VIEWPORTS: f32 = 1.0;

/// Stable identity and layout hints for a virtualized item.
///
/// Only `key` and `estimated_size` participate in the first Rust virtualizer.
/// The remaining hints stay in the Rust data model for Grid, sticky, and slot
/// recycling policies; none cross the Host boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemMeta<K> {
    key: K,
    reuse_identifier: Option<String>,
    estimated_size: Option<i32>,
    full_span: bool,
    sticky_top: bool,
    sticky_bottom: bool,
    recyclable: bool,
}

impl<K> ItemMeta<K> {
    /// Creates metadata with one stable logical key.
    pub fn key(key: K) -> Self {
        Self {
            key,
            reuse_identifier: None,
            estimated_size: None,
            full_span: false,
            sticky_top: false,
            sticky_bottom: false,
            recyclable: true,
        }
    }

    /// Groups items that may share a future recycled presentation slot.
    pub fn reuse_identifier(mut self, id: impl Into<String>) -> Self {
        self.reuse_identifier = Some(id.into());
        self
    }

    /// Sets the estimated main-axis size in logical pixels.
    pub fn estimated_size(mut self, px: i32) -> Self {
        self.estimated_size = Some(px);
        self
    }

    /// Marks an item as spanning the complete cross axis in Grid layouts.
    pub fn full_span(mut self, value: bool) -> Self {
        self.full_span = value;
        self
    }

    /// Marks an item as a leading-edge sticky candidate.
    pub fn sticky_top(mut self, value: bool) -> Self {
        self.sticky_top = value;
        self
    }

    /// Marks an item as a trailing-edge sticky candidate.
    pub fn sticky_bottom(mut self, value: bool) -> Self {
        self.sticky_bottom = value;
        self
    }

    /// Controls whether a future slot allocator may recycle this item.
    pub fn recyclable(mut self, value: bool) -> Self {
        self.recyclable = value;
        self
    }

    fn main_axis_size(&self) -> f32 {
        self.estimated_size
            .map(|value| value.max(0) as f32)
            .unwrap_or(DEFAULT_ITEM_SIZE)
    }
}

struct MountedEntry {
    owner: Owner,
    handle: Element,
}

struct RecycledEntry<T: 'static> {
    owner: Owner,
    handle: Element,
    item: RwSignal<T>,
    reuse_class: u32,
    recyclable: bool,
}

#[derive(Clone)]
struct RecyclePolicy {
    /// Interned within one List. Zero represents the default class (`None`).
    reuse_class: u32,
    recyclable: bool,
}

struct LayoutIndex<T, K> {
    items: Vec<T>,
    keys: Vec<K>,
    /// Prefix offsets. `offsets[i]` is the start of item `i` and the final
    /// entry is the complete estimated extent.
    offsets: Vec<f32>,
    /// Allocated only by the opt-in recycled-slot path. Plain `children:`
    /// lists do not pay per-item memory for future recycling metadata.
    recycle_policies: Option<Vec<RecyclePolicy>>,
    reuse_classes: Option<HashMap<String, u32>>,
    generation: u64,
}

impl<T, K> LayoutIndex<T, K>
where
    K: Eq + Hash + Clone,
{
    fn new(recycled_slots: bool) -> Self {
        Self {
            items: Vec::new(),
            keys: Vec::new(),
            offsets: vec![0.0],
            recycle_policies: recycled_slots.then(Vec::new),
            reuse_classes: recycled_slots.then(HashMap::new),
            generation: 0,
        }
    }

    fn replace(&mut self, items: Vec<T>, meta: impl Fn(&T) -> ItemMeta<K>) {
        let mut unique_keys = HashSet::with_capacity(items.len());
        self.keys.clear();
        self.keys.reserve(items.len());
        self.offsets.clear();
        self.offsets.reserve(items.len() + 1);
        self.offsets.push(0.0);
        if let Some(policies) = &mut self.recycle_policies {
            policies.clear();
            policies.reserve(items.len());
        }
        for item in &items {
            let item = meta(item);
            assert!(
                unique_keys.insert(item.key.clone()),
                "virtualized List keys must be unique"
            );
            let main_axis_size = item.main_axis_size();
            let reuse_class = if self.recycle_policies.is_some() {
                match item.reuse_identifier.as_ref() {
                    Some(identifier) => {
                        let classes = self
                            .reuse_classes
                            .as_mut()
                            .expect("recycled List must intern reuse identifiers");
                        if let Some(class) = classes.get(identifier) {
                            *class
                        } else {
                            let next = u32::try_from(classes.len() + 1)
                                .expect("List reuse identifier count exceeds u32");
                            classes.insert(identifier.clone(), next);
                            next
                        }
                    }
                    None => 0,
                }
            } else {
                0
            };
            if let Some(policies) = &mut self.recycle_policies {
                policies.push(RecyclePolicy {
                    reuse_class,
                    recyclable: item.recyclable,
                });
            }
            self.keys.push(item.key);
            self.offsets
                .push(self.offsets.last().copied().unwrap_or(0.0) + main_axis_size);
        }
        self.items = items;
        self.generation = self.generation.wrapping_add(1);
    }

    fn recycle_policy(&self, index: usize) -> &RecyclePolicy {
        &self
            .recycle_policies
            .as_ref()
            .expect("recycled List must retain slot policies")[index]
    }

    fn window(&self, geometry: ScrollGeometry) -> LayoutWindow {
        let item_count = self.items.len();
        let first_visible = self.offsets[1..]
            .partition_point(|end| *end <= geometry.offset)
            .min(item_count);
        let overscan_extent = geometry.viewport.max(0.0) * DEFAULT_OVERSCAN_VIEWPORTS;
        let overscan_start = (geometry.offset - overscan_extent).max(0.0);
        let first_in_overscan = self.offsets[1..]
            .partition_point(|end| *end <= overscan_start)
            .min(item_count);
        let start = first_in_overscan.min(first_visible.saturating_sub(DEFAULT_OVERSCAN_ITEMS));
        let visible_end = geometry.offset + geometry.viewport.max(0.0);
        let first_after_viewport = self.offsets[..item_count]
            .partition_point(|item_start| *item_start < visible_end)
            .max(first_visible.saturating_add(1).min(item_count));
        let total_extent = self.offsets.last().copied().unwrap_or(0.0);
        let overscan_end = (visible_end + overscan_extent).min(total_extent);
        let first_after_overscan =
            self.offsets[..item_count].partition_point(|item_start| *item_start < overscan_end);
        let end = first_after_overscan
            .max(first_after_viewport.saturating_add(DEFAULT_OVERSCAN_ITEMS))
            .min(item_count);

        LayoutWindow {
            generation: self.generation,
            start,
            end,
            leading_extent: self.offsets[start],
            trailing_extent: total_extent - self.offsets[end],
        }
    }
}

#[derive(Clone, Copy)]
struct LayoutWindow {
    generation: u64,
    start: usize,
    end: usize,
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
/// `each`, `meta`, and `children` mirror `ForEach`'s source, identity, and
/// item-builder split. Host `scroll` events update the mounted window;
/// reactive source changes run through the same reconciliation path.
pub fn virtualize<T, K>(
    scroll_view: Element,
    each: impl Fn() -> Vec<T> + 'static,
    meta: impl Fn(&T) -> ItemMeta<K> + 'static,
    children: impl Fn(T) -> Element + 'static,
) where
    T: Clone + 'static,
    K: Eq + Hash + Clone + 'static,
{
    let leading_spacer = create_element(ElementTag::View);
    let trailing_spacer = create_element(ElementTag::View);
    append_child(scroll_view, leading_spacer);
    append_child(scroll_view, trailing_spacer);

    let each = Rc::new(each);
    let meta = Rc::new(meta);
    let children = Rc::new(children);
    let geometry = Rc::new(RefCell::new(ScrollGeometry::default()));
    let layout = Rc::new(RefCell::new(LayoutIndex::<T, K>::new(false)));
    let mounted: Rc<RefCell<HashMap<K, MountedEntry>>> = Rc::new(RefCell::new(HashMap::new()));
    let rendered_window = Rc::new(Cell::new(None::<LayoutWindow>));

    let reconcile: Rc<dyn Fn()> = {
        let children = Rc::clone(&children);
        let geometry = Rc::clone(&geometry);
        let layout = Rc::clone(&layout);
        let mounted = Rc::clone(&mounted);
        let rendered_window = Rc::clone(&rendered_window);
        Rc::new(move || {
            let geometry = *geometry.borrow();
            let window = {
                let layout = layout.borrow();
                let window = layout.window(geometry);
                if rendered_window
                    .get()
                    .is_some_and(|rendered| rendered.identity() == window.identity())
                {
                    return;
                }
                window
            };

            set_spacer_size(leading_spacer, window.leading_extent);
            set_spacer_size(trailing_spacer, window.trailing_extent);

            let previous = rendered_window.get();
            if let Some(previous) = previous.filter(|old| old.generation == window.generation) {
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
                        remove_child(scroll_view, entry.handle);
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
                        let owner = Owner::new(None);
                        let handle = owner.with(|| children(layout.items[index].clone()));
                        let replaced = mounted.insert(key, MountedEntry { owner, handle });
                        debug_assert!(replaced.is_none());
                        insert_child_at(scroll_view, handle, index - window.start + 1);
                    },
                );
                rendered_window.set(Some(window));
                return;
            }

            // A source replacement may reorder keys arbitrarily. Preserve keyed
            // entries still in the new window and dispose the rest.
            let mut order = Vec::with_capacity(window.end - window.start + 2);
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
                        remove_child(scroll_view, entry.handle);
                        entry.owner.dispose();
                    }
                }

                for index in window.start..window.end {
                    let key = &layout.keys[index];
                    if !mounted.contains_key(key) {
                        let owner = Owner::new(None);
                        let handle = owner.with(|| children(layout.items[index].clone()));
                        mounted.insert(key.clone(), MountedEntry { owner, handle });
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
            sync_child_order(scroll_view, &order);
            rendered_window.set(Some(window));
        })
    };

    {
        let each = Rc::clone(&each);
        let meta = Rc::clone(&meta);
        let layout = Rc::clone(&layout);
        let reconcile = Rc::clone(&reconcile);
        effect(move || {
            let items = each();
            layout.borrow_mut().replace(items, |item| meta(item));
            reconcile();
        });
    }

    {
        let geometry = Rc::clone(&geometry);
        let reconcile = Rc::clone(&reconcile);
        set_event_listener(
            scroll_view,
            "scroll",
            BindType::Bind,
            Box::new(move |event| {
                if let Some(next) = scroll_geometry(&event) {
                    *geometry.borrow_mut() = next;
                    reconcile();
                }
            }),
        );
    }

    on_cleanup(move || {
        for (_, entry) in mounted.borrow_mut().drain() {
            remove_child(scroll_view, entry.handle);
            entry.owner.dispose();
        }
        remove_child(scroll_view, leading_spacer);
        remove_child(scroll_view, trailing_spacer);
    });
}

/// Installs the opt-in recycled-slot List path.
///
/// Unlike [`virtualize`], the item builder receives a stable signal. Rows that
/// leave and enter during one reconciliation exchange slots with a matching
/// `reuse_identifier`; updating the signal rebinds reactive props while the
/// element subtree and Host views keep their identity. No detached slots are
/// retained across frames, so scene size remains bounded by the mounted window.
pub fn virtualize_recycled<T, K>(
    scroll_view: Element,
    each: impl Fn() -> Vec<T> + 'static,
    meta: impl Fn(&T) -> ItemMeta<K> + 'static,
    children: impl Fn(ReadSignal<T>) -> Element + 'static,
) where
    T: Clone + 'static,
    K: Eq + Hash + Clone + 'static,
{
    let leading_spacer = create_element(ElementTag::View);
    let trailing_spacer = create_element(ElementTag::View);
    append_child(scroll_view, leading_spacer);
    append_child(scroll_view, trailing_spacer);

    let each = Rc::new(each);
    let meta = Rc::new(meta);
    let children: Rc<dyn Fn(ReadSignal<T>) -> Element> = Rc::new(children);
    let geometry = Rc::new(RefCell::new(ScrollGeometry::default()));
    let layout = Rc::new(RefCell::new(LayoutIndex::<T, K>::new(true)));
    let mounted: Rc<RefCell<HashMap<K, RecycledEntry<T>>>> = Rc::new(RefCell::new(HashMap::new()));
    let rendered_window = Rc::new(Cell::new(None::<LayoutWindow>));

    let reconcile: Rc<dyn Fn()> = {
        let children = Rc::clone(&children);
        let geometry = Rc::clone(&geometry);
        let layout = Rc::clone(&layout);
        let mounted = Rc::clone(&mounted);
        let rendered_window = Rc::clone(&rendered_window);
        Rc::new(move || {
            let geometry = *geometry.borrow();
            let window = {
                let layout = layout.borrow();
                let window = layout.window(geometry);
                if rendered_window
                    .get()
                    .is_some_and(|rendered| rendered.identity() == window.identity())
                {
                    return;
                }
                window
            };

            set_spacer_size(leading_spacer, window.leading_extent);
            set_spacer_size(trailing_spacer, window.trailing_extent);

            let previous = rendered_window.get();
            if let Some(previous) = previous.filter(|old| old.generation == window.generation) {
                let layout = layout.borrow();
                let mut mounted = mounted.borrow_mut();
                let mut pool = Vec::new();

                visit_range_difference(
                    previous.start,
                    previous.end,
                    window.start,
                    window.end,
                    |index| {
                        let entry = mounted
                            .remove(&layout.keys[index])
                            .expect("a leaving recycled List row must be mounted");
                        remove_child(scroll_view, entry.handle);
                        if entry.recyclable {
                            pool.push(entry);
                        } else {
                            entry.owner.dispose();
                        }
                    },
                );
                visit_range_difference(
                    window.start,
                    window.end,
                    previous.start,
                    previous.end,
                    |index| {
                        let policy = layout.recycle_policy(index);
                        let item = layout.items[index].clone();
                        let entry = if let Some(mut entry) = take_recycled_entry(&mut pool, policy)
                        {
                            entry.item.set(item);
                            entry.recyclable = policy.recyclable;
                            entry
                        } else {
                            new_recycled_entry(item, policy, children.as_ref())
                        };
                        let handle = entry.handle;
                        let replaced = mounted.insert(layout.keys[index].clone(), entry);
                        debug_assert!(replaced.is_none());
                        insert_child_at(scroll_view, handle, index - window.start + 1);
                    },
                );
                for entry in pool {
                    entry.owner.dispose();
                }
                rendered_window.set(Some(window));
                return;
            }

            let layout = layout.borrow();
            let mut mounted = mounted.borrow_mut();
            let mut pool = Vec::new();
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
                let entry = mounted
                    .remove(&key)
                    .expect("a stale recycled List row must be mounted");
                remove_child(scroll_view, entry.handle);
                if entry.recyclable {
                    pool.push(entry);
                } else {
                    entry.owner.dispose();
                }
            }

            let mut order = Vec::with_capacity(window.end - window.start + 2);
            order.push(leading_spacer);
            for index in window.start..window.end {
                let key = &layout.keys[index];
                let policy = layout.recycle_policy(index);
                let incompatible = mounted
                    .get(key)
                    .is_some_and(|entry| entry.reuse_class != policy.reuse_class);
                if incompatible {
                    let entry = mounted
                        .remove(key)
                        .expect("the incompatible recycled row must be mounted");
                    remove_child(scroll_view, entry.handle);
                    if entry.recyclable {
                        pool.push(entry);
                    } else {
                        entry.owner.dispose();
                    }
                }

                if let Some(entry) = mounted.get_mut(key) {
                    entry.item.set(layout.items[index].clone());
                    entry.recyclable = policy.recyclable;
                } else {
                    let item = layout.items[index].clone();
                    let entry = if let Some(mut entry) = take_recycled_entry(&mut pool, policy) {
                        entry.item.set(item);
                        entry.recyclable = policy.recyclable;
                        entry
                    } else {
                        new_recycled_entry(item, policy, children.as_ref())
                    };
                    mounted.insert(key.clone(), entry);
                }
                order.push(
                    mounted
                        .get(key)
                        .expect("the desired recycled List row must be mounted")
                        .handle,
                );
            }
            order.push(trailing_spacer);
            sync_child_order(scroll_view, &order);
            for entry in pool {
                entry.owner.dispose();
            }
            rendered_window.set(Some(window));
        })
    };

    {
        let each = Rc::clone(&each);
        let meta = Rc::clone(&meta);
        let layout = Rc::clone(&layout);
        let reconcile = Rc::clone(&reconcile);
        effect(move || {
            let items = each();
            layout.borrow_mut().replace(items, |item| meta(item));
            reconcile();
        });
    }

    {
        let geometry = Rc::clone(&geometry);
        let reconcile = Rc::clone(&reconcile);
        set_event_listener(
            scroll_view,
            "scroll",
            BindType::Bind,
            Box::new(move |event| {
                if let Some(next) = scroll_geometry(&event) {
                    *geometry.borrow_mut() = next;
                    reconcile();
                }
            }),
        );
    }

    on_cleanup(move || {
        for (_, entry) in mounted.borrow_mut().drain() {
            remove_child(scroll_view, entry.handle);
            entry.owner.dispose();
        }
        remove_child(scroll_view, leading_spacer);
        remove_child(scroll_view, trailing_spacer);
    });
}

fn new_recycled_entry<T: Clone + 'static>(
    item: T,
    policy: &RecyclePolicy,
    children: &dyn Fn(ReadSignal<T>) -> Element,
) -> RecycledEntry<T> {
    let owner = Owner::new(None);
    let (item, handle) = owner.with(|| {
        let item = RwSignal::new(item);
        let handle = children(item.read_only());
        (item, handle)
    });
    RecycledEntry {
        owner,
        handle,
        item,
        reuse_class: policy.reuse_class,
        recyclable: policy.recyclable,
    }
}

fn take_recycled_entry<T: 'static>(
    pool: &mut Vec<RecycledEntry<T>>,
    policy: &RecyclePolicy,
) -> Option<RecycledEntry<T>> {
    if !policy.recyclable {
        return None;
    }
    let position = pool
        .iter()
        .rposition(|entry| entry.reuse_class == policy.reuse_class)?;
    Some(pool.swap_remove(position))
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
}

fn set_spacer_size(element: Element, size: f32) {
    let px = LengthPercentageValue::Length(if size == 0.0 {
        LengthValue::Zero
    } else {
        LengthValue::Dimension {
            value: StyleNumber::new(size),
            unit: LengthUnit::Px,
        }
    });
    let style = SpecifiedStyle::new()
        .push(
            StyleProperty::Height,
            StyleValue::Size(SizeValue::LengthPercentage(px)),
        )
        .push(
            StyleProperty::FlexShrink,
            StyleValue::Number(StyleNumber::new(0.0)),
        );
    set_specified_style(element, &style);
}

fn scroll_geometry(event: &WhiskerValue) -> Option<ScrollGeometry> {
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
    Some(ScrollGeometry {
        offset: number("scrollTop")?.max(0.0),
        viewport: number("viewportHeight")?.max(0.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn recycled_index_interns_reuse_identifiers_once() {
        let mut index = LayoutIndex::new(true);
        index.replace(vec![0_u32, 1, 2], |item| {
            ItemMeta::key(*item).reuse_identifier(if item & 1 == 0 { "even" } else { "odd" })
        });

        assert_eq!(index.reuse_classes.as_ref().unwrap().len(), 2);
        let policies = index.recycle_policies.as_ref().unwrap();
        assert_eq!(policies[0].reuse_class, policies[2].reuse_class);
        assert_ne!(policies[0].reuse_class, policies[1].reuse_class);

        let mut plain = LayoutIndex::new(false);
        plain.replace(vec![0_u32], |item| {
            ItemMeta::key(*item).reuse_identifier("ignored")
        });
        assert!(plain.recycle_policies.is_none());
        assert!(plain.reuse_classes.is_none());
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
        let geometry = scroll_geometry(&event).unwrap();
        assert_eq!(geometry.offset, 120.0);
        assert_eq!(geometry.viewport, 480.0);
    }

    #[test]
    fn window_keeps_one_viewport_of_scroll_buffer_on_each_side() {
        let mut index = LayoutIndex::new(false);
        index.replace((0_u32..10_000).collect(), |item| {
            ItemMeta::key(*item).estimated_size(72)
        });

        let geometry = ScrollGeometry {
            offset: 72_000.0,
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
}
