use std::cell::RefCell;

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk, pango};
use gtk4 as gtk;
use whisker_protocol::{
    BackgroundLayer, BoxClip, BoxPaint, CursorKeyword, LayoutGeometry, OverflowClip, Transform,
    Visibility, VisualEffects,
};

use crate::paint;

/// Text painted from the exact Pango layout accepted by measurement.
#[derive(Clone)]
pub(crate) struct PaintedText {
    pub(crate) layout: pango::Layout,
    pub(crate) color: gdk::RGBA,
    /// Inline alignment offset for unwrapped text inside the content box.
    pub(crate) offset: [f32; 2],
    pub(crate) shadows: Vec<gsk::Shadow>,
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
    pub(crate) layers: Vec<BackgroundLayer>,
    pub(crate) effects: VisualEffects,
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
            layers: Vec::new(),
            effects: VisualEffects::default(),
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
            transform = apply_matrix(transform, matrix.0);
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
        let clip_path = state.effects.clip_path.as_ref().map(|(reference, shape)| {
            paint::clip_path(state.paint.as_ref(), &state.geometry, *reference, shape)
        });
        if let Some((path, fill_rule)) = &clip_path {
            snapshot.push_fill(path, *fill_rule);
        }
        if let Some(background) = state.background {
            let bounds = graphene::Rect::new(0.0, 0.0, self.width() as f32, self.height() as f32);
            snapshot.append_color(&background, &bounds);
        }
        let rect = state.geometry.border_box;
        let bounds = graphene::Rect::new(0.0, 0.0, rect.width, rect.height);
        let visible = state.visibility == Visibility::Visible;
        let paint = state.paint.as_ref();
        if visible {
            let shadows = &state.effects.box_shadows;
            paint::paint_box_shadows(snapshot, shadows, paint, &state.geometry, false);
            paint::paint_background(snapshot, paint, &state.layers, &state.geometry);
            paint::paint_box_shadows(snapshot, shadows, paint, &state.geometry, true);
            if let Some(paint) = paint {
                paint::paint_border(snapshot, paint, &state.geometry);
            }
            if let Some(text) = &state.text {
                paint_text(snapshot, text, &state.geometry);
            }
        }
        let clip = clip_rect(&state, &bounds);
        let outline = visible
            .then(|| state.effects.outline.clone())
            .flatten()
            .map(|outline| (outline, state.paint.clone()));
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
        if let Some((outline, paint)) = outline {
            paint::paint_outline(snapshot, &outline, paint.as_ref(), &bounds);
        }
        if clip_path.is_some() {
            snapshot.pop();
        }
        if translucent {
            snapshot.pop();
        }
    }
}

// GSK categorizes any raw matrix as 3D, which the Cairo renderer cannot draw.
fn apply_matrix(transform: gsk::Transform, m: [f32; 16]) -> gsk::Transform {
    let affine = [m[2], m[3], m[6], m[7], m[8], m[9], m[11], m[14]] == [0.0; 8]
        && m[10] == 1.0
        && m[15] == 1.0;
    if !affine {
        return transform.matrix(&graphene::Matrix::from_float(m));
    }
    let [a, b, c, d, tx, ty] = [m[0], m[1], m[4], m[5], m[12], m[13]];
    let scale_x = a.hypot(b);
    let angle = b.atan2(a);
    let (sin, cos) = angle.sin_cos();
    let shear = c * cos + d * sin;
    let scale_y = d * cos - c * sin;
    let transform = transform
        .translate(&graphene::Point::new(tx, ty))
        .rotate(angle.to_degrees());
    let transform = if scale_y == 0.0 {
        transform
    } else {
        transform.skew((shear / scale_y).atan().to_degrees(), 0.0)
    };
    transform.scale(scale_x, scale_y)
}

fn paint_text(snapshot: &gtk::Snapshot, text: &PaintedText, geometry: &LayoutGeometry) {
    let content = geometry.content_box;
    snapshot.save();
    snapshot.translate(&graphene::Point::new(
        content.x + text.offset[0],
        content.y + text.offset[1],
    ));
    if !text.shadows.is_empty() {
        snapshot.push_shadow(&text.shadows);
    }
    snapshot.append_layout(&text.layout, &text.color);
    if !text.shadows.is_empty() {
        snapshot.pop();
    }
    snapshot.restore();
}

fn clip_rect(state: &NodeState, bounds: &graphene::Rect) -> Option<gsk::RoundedRect> {
    let scrolls = state.scroll.is_some();
    let horizontal = scrolls || state.clip.horizontal == OverflowClip::Hidden;
    let vertical = scrolls || state.clip.vertical == OverflowClip::Hidden;
    if !horizontal && !vertical {
        return None;
    }
    if horizontal && vertical {
        return Some(paint::rounded_rect(state.paint.as_ref(), bounds));
    }
    const UNBOUNDED: f32 = 1.0e6;
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
