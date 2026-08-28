//! Rust-owned windowing for the built-in `list` control primitive.
//!
//! A list is deliberately not a Host element. The authoring builder creates
//! the ordinary built-in `ScrollView`; this module keeps only a bounded set of
//! ordinary item subtrees mounted below it and uses two presentation-only
//! spacer Views to preserve the complete scroll extent. Hosts therefore need
//! no list-specific ABI, view class, recycling contract, or data source.

use std::cell::RefCell;
use std::collections::HashMap;
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
    let mounted: Rc<RefCell<HashMap<K, MountedEntry>>> = Rc::new(RefCell::new(HashMap::new()));

    let reconcile: Rc<dyn Fn()> = {
        let each = Rc::clone(&each);
        let meta = Rc::clone(&meta);
        let children = Rc::clone(&children);
        let geometry = Rc::clone(&geometry);
        let mounted = Rc::clone(&mounted);
        Rc::new(move || {
            let items = each();
            let metadata = items.iter().map(|item| meta(item)).collect::<Vec<_>>();
            let sizes = metadata
                .iter()
                .map(ItemMeta::main_axis_size)
                .collect::<Vec<_>>();
            let geometry = *geometry.borrow();

            let mut cursor = 0.0;
            let first_visible = sizes
                .iter()
                .position(|size| {
                    let visible = cursor + *size > geometry.offset;
                    cursor += *size;
                    visible
                })
                .unwrap_or(sizes.len());
            let start = first_visible.saturating_sub(DEFAULT_OVERSCAN_ITEMS);
            let visible_end = geometry.offset + geometry.viewport.max(0.0);
            let mut extent = sizes.iter().take(start).sum::<f32>();
            let mut end = start;
            while end < sizes.len()
                && (extent < visible_end || end < first_visible.saturating_add(1))
            {
                extent += sizes[end];
                end += 1;
            }
            end = end.saturating_add(DEFAULT_OVERSCAN_ITEMS).min(sizes.len());

            set_spacer_size(leading_spacer, sizes[..start].iter().sum());
            set_spacer_size(trailing_spacer, sizes[end..].iter().sum());

            let mut old = std::mem::take(&mut *mounted.borrow_mut());
            let mut next = HashMap::with_capacity(end.saturating_sub(start));
            let mut order = Vec::with_capacity(end.saturating_sub(start));
            for index in start..end {
                let key = metadata[index].key.clone();
                let entry = if let Some(entry) = old.remove(&key) {
                    entry
                } else {
                    let owner = Owner::new(None);
                    let handle = owner.with(|| children(items[index].clone()));
                    MountedEntry { owner, handle }
                };
                order.push((key.clone(), entry.handle));
                next.insert(key, entry);
            }

            for (_, entry) in old.drain() {
                remove_child(scroll_view, entry.handle);
                entry.owner.dispose();
            }
            let attached = children_of(scroll_view);
            for entry in next.values() {
                if attached.contains(&entry.handle) {
                    remove_child(scroll_view, entry.handle);
                }
            }
            for (index, (_, handle)) in order.into_iter().enumerate() {
                insert_child_at(scroll_view, handle, index + 1);
            }
            *mounted.borrow_mut() = next;
        })
    };

    {
        let reconcile = Rc::clone(&reconcile);
        effect(move || reconcile());
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
