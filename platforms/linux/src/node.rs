use std::cell::RefCell;

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk, pango};
use gtk4 as gtk;
use whisker_protocol::{
    BorderLineStyle, BoxClip, BoxPaint, CursorKeyword, LayoutGeometry, OverflowClip,
    PaintCornerRadius, PaintLengthPercentage, Transform, Visibility,
};

use crate::color;

/// Text painted from the exact Pango layout accepted by measurement.
#[derive(Clone)]
pub(crate) struct PaintedText {
    pub(crate) layout: pango::Layout,
    pub(crate) color: gdk::RGBA,
    /// Inline alignment offset for unwrapped text inside the content box.
    pub(crate) offset: [f32; 2],
}

/// Native scroll position of a ScrollView node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScrollState {
    pub(crate) horizontal: bool,
    pub(crate) enabled: bool,
    pub(crate) offset: [f32; 2],
}

impl Default for ScrollState {
    fn default() -> Self {
        Self {
            horizontal: false,
            enabled: true,
            offset: [0.0, 0.0],
        }
    }
}

pub(crate) struct NodeState {
    pub(crate) id: Option<whisker_protocol::NodeId>,
    pub(crate) geometry: LayoutGeometry,
    pub(crate) paint: Option<BoxPaint>,
    pub(crate) clip: BoxClip,
    pub(crate) transform: Option<Transform>,
    pub(crate) opacity: f32,
    pub(crate) visibility: Visibility,
    pub(crate) z_order: i32,
    pub(crate) text: Option<PaintedText>,
    pub(crate) scroll: Option<ScrollState>,
    pub(crate) background: Option<gdk::RGBA>,
    pub(crate) cursor: CursorKeyword,
    pub(crate) children: Vec<NodeWidget>,
    on_resize: Option<fn()>,
    allocated: (i32, i32),
}

impl Default for NodeState {
    fn default() -> Self {
        Self {
            id: None,
            geometry: LayoutGeometry::default(),
            paint: None,
            clip: BoxClip {
                horizontal: OverflowClip::Visible,
                vertical: OverflowClip::Visible,
            },
            transform: None,
            opacity: 1.0,
            visibility: Visibility::Visible,
            z_order: 0,
            text: None,
            scroll: None,
            background: None,
            cursor: CursorKeyword::Auto,
            children: Vec::new(),
            on_resize: None,
            allocated: (0, 0),
        }
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct NodeWidget {
        pub(super) state: RefCell<NodeState>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for NodeWidget {
        const NAME: &'static str = "WhiskerNode";
        type Type = super::NodeWidget;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for NodeWidget {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().set_overflow(gtk::Overflow::Visible);
        }

        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for NodeWidget {
        // A zero minimum keeps GTK from overriding Rust-computed geometry.
        fn measure(&self, _orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            (0, 0, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let resized = {
                let mut state = self.state.borrow_mut();
                let previous = std::mem::replace(&mut state.allocated, (width, height));
                state.on_resize.filter(|_| previous != (width, height))
            };
            if let Some(on_resize) = resized {
                on_resize();
            }
            let state = self.state.borrow();
            let offset = state.scroll.map_or([0.0, 0.0], |scroll| scroll.offset);
            for child in &state.children {
                child.allocate_in_parent(offset);
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            self.obj().paint(snapshot);
        }
    }
}

glib::wrapper! {
    pub struct NodeWidget(ObjectSubclass<imp::NodeWidget>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for NodeWidget {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl NodeWidget {
    pub(crate) fn new() -> Self {
        let widget = Self::default();
        widget.set_can_target(false);
        widget
    }

    /// Calls `callback` whenever GTK allocates this widget a new size.
    pub(crate) fn set_resize_callback(&self, callback: fn()) {
        self.state_mut().on_resize = Some(callback);
    }

    pub(crate) fn state(&self) -> std::cell::Ref<'_, NodeState> {
        self.imp().state.borrow()
    }

    pub(crate) fn state_mut(&self) -> std::cell::RefMut<'_, NodeState> {
        self.imp().state.borrow_mut()
    }

    fn allocate_in_parent(&self, parent_scroll: [f32; 2]) {
        let state = self.state();
        let rect = state.geometry.border_box;
        let mut transform = gsk::Transform::new().translate(&graphene::Point::new(
            rect.x - parent_scroll[0],
            rect.y - parent_scroll[1],
        ));
        if let Some(matrix) = state.transform {
            transform = transform.matrix(&graphene::Matrix::from_float(matrix.0));
        }
        let (width, height) = (rect.width.ceil() as i32, rect.height.ceil() as i32);
        drop(state);
        self.allocate(width, height, -1, Some(transform));
    }

    /// Children in paint order: ascending z-order, then document order.
    fn paint_order(&self) -> Vec<NodeWidget> {
        let mut children = self.state().children.clone();
        children.sort_by_key(|child| child.state().z_order);
        children
    }

    fn paint(&self, snapshot: &gtk::Snapshot) {
        let state = self.state();
        let translucent = state.opacity < 1.0;
        if translucent {
            snapshot.push_opacity(f64::from(state.opacity.max(0.0)));
        }
        if let Some(background) = state.background {
            let bounds = graphene::Rect::new(0.0, 0.0, self.width() as f32, self.height() as f32);
            snapshot.append_color(&background, &bounds);
        }
        let rect = state.geometry.border_box;
        let bounds = graphene::Rect::new(0.0, 0.0, rect.width, rect.height);
        if state.visibility == Visibility::Visible {
            if let Some(paint) = &state.paint {
                paint_box(snapshot, paint, &bounds);
            }
            if let Some(text) = &state.text {
                let content = state.geometry.content_box;
                snapshot.save();
                snapshot.translate(&graphene::Point::new(
                    content.x + text.offset[0],
                    content.y + text.offset[1],
                ));
                snapshot.append_layout(&text.layout, &text.color);
                snapshot.restore();
            }
        }
        let clip = clip_rect(&state, &bounds);
        drop(state);
        if let Some(clip) = &clip {
            snapshot.push_rounded_clip(clip);
        }
        for child in self.paint_order() {
            self.snapshot_child(&child, snapshot);
        }
        if clip.is_some() {
            snapshot.pop();
        }
        if translucent {
            snapshot.pop();
        }
    }
}

fn resolve(value: PaintLengthPercentage, basis: f32) -> f32 {
    value.length + value.fraction * basis
}

fn corner(radius: PaintCornerRadius, bounds: &graphene::Rect) -> graphene::Size {
    graphene::Size::new(
        resolve(radius.horizontal, bounds.width()),
        resolve(radius.vertical, bounds.height()),
    )
}

pub(crate) fn rounded_rect(paint: Option<&BoxPaint>, bounds: &graphene::Rect) -> gsk::RoundedRect {
    let Some(paint) = paint else {
        return gsk::RoundedRect::from_rect(*bounds, 0.0);
    };
    let radii = &paint.border_radii;
    let mut rect = gsk::RoundedRect::new(
        *bounds,
        corner(radii.top_left, bounds),
        corner(radii.top_right, bounds),
        corner(radii.bottom_right, bounds),
        corner(radii.bottom_left, bounds),
    );
    rect.normalize();
    rect
}

fn paint_box(snapshot: &gtk::Snapshot, paint: &BoxPaint, bounds: &graphene::Rect) {
    let outline = rounded_rect(Some(paint), bounds);
    let background = color::rgba(&paint.background_color);
    if !color::is_transparent(&background) {
        if outline.is_rectilinear() {
            snapshot.append_color(&background, bounds);
        } else {
            snapshot.push_rounded_clip(&outline);
            snapshot.append_color(&background, bounds);
            snapshot.pop();
        }
    }
    let edges = [
        (
            paint.border_widths.top,
            paint.border_styles.top,
            &paint.border_colors.top,
        ),
        (
            paint.border_widths.right,
            paint.border_styles.right,
            &paint.border_colors.right,
        ),
        (
            paint.border_widths.bottom,
            paint.border_styles.bottom,
            &paint.border_colors.bottom,
        ),
        (
            paint.border_widths.left,
            paint.border_styles.left,
            &paint.border_colors.left,
        ),
    ];
    let widths = edges.map(|(width, style, _)| {
        if matches!(style, BorderLineStyle::None | BorderLineStyle::Hidden) {
            0.0
        } else {
            resolve(width, bounds.width())
        }
    });
    if widths.iter().any(|width| *width > 0.0) {
        let colors = edges.map(|(_, _, color)| color::rgba(color));
        snapshot.append_border(&outline, &widths, &colors);
    }
}

fn clip_rect(state: &NodeState, bounds: &graphene::Rect) -> Option<gsk::RoundedRect> {
    let scrolls = state.scroll.is_some();
    let horizontal = scrolls || state.clip.horizontal == OverflowClip::Hidden;
    let vertical = scrolls || state.clip.vertical == OverflowClip::Hidden;
    if !horizontal && !vertical {
        return None;
    }
    if horizontal && vertical {
        return Some(rounded_rect(state.paint.as_ref(), bounds));
    }
    const UNBOUNDED: f32 = 1.0e7;
    let (x, width) = if horizontal {
        (bounds.x(), bounds.width())
    } else {
        (-UNBOUNDED, 2.0 * UNBOUNDED)
    };
    let (y, height) = if vertical {
        (bounds.y(), bounds.height())
    } else {
        (-UNBOUNDED, 2.0 * UNBOUNDED)
    };
    Some(gsk::RoundedRect::from_rect(
        graphene::Rect::new(x, y, width, height),
        0.0,
    ))
}
