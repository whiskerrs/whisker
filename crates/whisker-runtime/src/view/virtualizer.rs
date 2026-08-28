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
use crate::reactive::{Owner, effect, on_cleanup};

use super::handle::Element;
use super::renderer::{
    BindType, append_child, children_of, create_element, insert_child_at, remove_child,
    set_event_listener, set_specified_style,
};

const DEFAULT_ITEM_SIZE: f32 = 44.0;
const DEFAULT_VIEWPORT_SIZE: f32 = 600.0;
const DEFAULT_OVERSCAN_ITEMS: usize = 2;

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

struct LayoutIndex<T, K> {
    items: Vec<T>,
    keys: Vec<K>,
    /// Prefix offsets. `offsets[i]` is the start of item `i` and the final
    /// entry is the complete estimated extent.
    offsets: Vec<f32>,
    generation: u64,
}

impl<T, K> LayoutIndex<T, K>
where
    K: Eq + Hash + Clone,
{
    fn new() -> Self {
        Self {
            items: Vec::new(),
            keys: Vec::new(),
            offsets: vec![0.0],
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
        for item in &items {
            let item = meta(item);
            assert!(
                unique_keys.insert(item.key.clone()),
                "virtualized List keys must be unique"
            );
            let main_axis_size = item.main_axis_size();
            self.keys.push(item.key);
            self.offsets
                .push(self.offsets.last().copied().unwrap_or(0.0) + main_axis_size);
        }
        self.items = items;
        self.generation = self.generation.wrapping_add(1);
    }

    fn window(&self, geometry: ScrollGeometry) -> LayoutWindow {
        let item_count = self.items.len();
        let first_visible = self.offsets[1..]
            .partition_point(|end| *end <= geometry.offset)
            .min(item_count);
        let start = first_visible.saturating_sub(DEFAULT_OVERSCAN_ITEMS);
        let visible_end = geometry.offset + geometry.viewport.max(0.0);
        let first_after_viewport = self.offsets[..item_count]
            .partition_point(|item_start| *item_start < visible_end)
            .max(first_visible.saturating_add(1).min(item_count));
        let end = first_after_viewport
            .saturating_add(DEFAULT_OVERSCAN_ITEMS)
            .min(item_count);
        let total_extent = self.offsets.last().copied().unwrap_or(0.0);

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
    let layout = Rc::new(RefCell::new(LayoutIndex::<T, K>::new()));
    let mounted: Rc<RefCell<HashMap<K, MountedEntry>>> = Rc::new(RefCell::new(HashMap::new()));
    let rendered_window = Rc::new(Cell::new(None::<(u64, usize, usize)>));

    let reconcile: Rc<dyn Fn()> = {
        let children = Rc::clone(&children);
        let geometry = Rc::clone(&geometry);
        let layout = Rc::clone(&layout);
        let mounted = Rc::clone(&mounted);
        let rendered_window = Rc::clone(&rendered_window);
        Rc::new(move || {
            let geometry = *geometry.borrow();
            let (window, desired) = {
                let layout = layout.borrow();
                let window = layout.window(geometry);
                if rendered_window.get() == Some(window.identity()) {
                    return;
                }
                let desired = (window.start..window.end)
                    .map(|index| (layout.keys[index].clone(), layout.items[index].clone()))
                    .collect::<Vec<_>>();
                (window, desired)
            };

            set_spacer_size(leading_spacer, window.leading_extent);
            set_spacer_size(trailing_spacer, window.trailing_extent);

            let mut old = std::mem::take(&mut *mounted.borrow_mut());
            let mut next = HashMap::with_capacity(desired.len());
            let mut order = Vec::with_capacity(desired.len() + 2);
            order.push(leading_spacer);
            for (key, item) in desired {
                let entry = if let Some(entry) = old.remove(&key) {
                    entry
                } else {
                    let owner = Owner::new(None);
                    let handle = owner.with(|| children(item));
                    MountedEntry { owner, handle }
                };
                order.push(entry.handle);
                next.insert(key, entry);
            }
            order.push(trailing_spacer);

            for (_, entry) in old.drain() {
                remove_child(scroll_view, entry.handle);
                entry.owner.dispose();
            }
            sync_child_order(scroll_view, &order);
            *mounted.borrow_mut() = next;
            rendered_window.set(Some(window.identity()));
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
}
