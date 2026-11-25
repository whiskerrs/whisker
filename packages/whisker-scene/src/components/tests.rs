use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use glam::{Quat, Vec3};
use whisker::Owner;
use whisker::runtime::reactive::{__reset_for_tests, __reset_pending_mount_for_tests};
use whisker::runtime::view::{
    BindType, DynRenderer, Element, install_renderer, uninstall_renderer,
};
use whisker_surface::FrameInfo;

use super::*;
use crate::core::{Geometry, Material, Node, Scene};

#[derive(Default)]
struct Recorder {
    next: Cell<u32>,
}

impl DynRenderer for Recorder {
    fn create_element(&self, _tag: ElementTag) -> Element {
        self.create_element_by_name("")
    }
    fn create_element_by_name(&self, _tag_name: &str) -> Element {
        let id = self.next.get();
        self.next.set(id + 1);
        Element::from_raw(id)
    }
    fn release_element(&self, _: Element) {}
    fn set_attribute(&self, _: Element, _: &str, _: &str) {}
    fn append_child(&self, _: Element, _: Element) {}
    fn remove_child(&self, _: Element, _: Element) {}
    fn set_event_listener(
        &self,
        _: Element,
        _: &str,
        _: BindType,
        _: Box<dyn Fn(whisker::WhiskerValue) + 'static>,
    ) {
    }
    fn set_root(&self, _: Element) {}
    fn flush(&self) {}
}

/// Mounts `view` under a fresh owner, runs `check`, then disposes the owner.
fn mounted(view: impl FnOnce() -> Element, check: impl FnOnce(&Owner)) {
    __reset_for_tests();
    __reset_pending_mount_for_tests();
    let previous = install_renderer(Box::new(Recorder::default()));
    let owner = Owner::new(None);
    owner.with(|| {
        view();
        check(&owner);
    });
    owner.dispose();
    uninstall_renderer(previous);
}

fn cube() -> Geometry {
    Geometry::cuboid(Vec3::ONE)
}

fn red() -> Material {
    Material::Unlit {
        color: Color::rgb(255, 0, 0),
    }
}

#[test]
fn nodes_follow_the_component_tree() {
    let scene = Scene::new();
    let shown = scene.clone();
    mounted(
        move || {
            render! {
                SceneView(style: Css::new(), scene: shown) {
                    Group(position: Vec3::X) {
                        Mesh(geometry: cube(), material: red(), position: Vec3::Y)
                    }
                    Mesh(geometry: cube(), material: red())
                }
            }
        },
        |_| {
            let top = scene.root().children();
            assert_eq!(top.len(), 2);
            assert_eq!(top[0].transform().translation, Vec3::X);
            let nested = top[0].children();
            assert_eq!(nested.len(), 1);
            assert_eq!(nested[0].transform().translation, Vec3::Y);
            assert!(nested[0].mesh().is_some());
        },
    );
    assert!(scene.root().children().is_empty());
}

#[test]
fn signal_props_update_the_node() {
    let scene = Scene::new();
    let shown = scene.clone();
    let angle = Rc::new(RefCell::new(None::<RwSignal<f32>>));
    let angle_slot = angle.clone();
    mounted(
        move || {
            let angle = signal(0.0f32);
            *angle_slot.borrow_mut() = Some(angle);
            render! {
                SceneView(style: Css::new(), scene: shown) {
                    Mesh(
                        geometry: cube(),
                        material: red(),
                        rotation: computed(move || Quat::from_rotation_y(angle.get())),
                    )
                }
            }
        },
        |_| {
            let angle = angle.borrow().unwrap();
            angle.set(1.0);
            whisker::flush();
            let node = &scene.root().children()[0];
            assert!(
                node.transform()
                    .rotation
                    .abs_diff_eq(Quat::from_rotation_y(1.0), 1e-6)
            );
        },
    );
}

#[test]
fn show_adds_and_removes_nodes() {
    let scene = Scene::new();
    let shown = scene.clone();
    let visible = Rc::new(RefCell::new(None::<RwSignal<bool>>));
    let visible_slot = visible.clone();
    mounted(
        move || {
            let visible = signal(true);
            *visible_slot.borrow_mut() = Some(visible);
            render! {
                SceneView(style: Css::new(), scene: shown) {
                    Show(when: move || visible.get()) {
                        Mesh(geometry: cube(), material: red())
                    }
                }
            }
        },
        |_| {
            let visible = visible.borrow().unwrap();
            assert_eq!(scene.root().children().len(), 1);
            visible.set(false);
            whisker::flush();
            assert!(scene.root().children().is_empty());
            visible.set(true);
            whisker::flush();
            assert_eq!(scene.root().children().len(), 1);
        },
    );
}

#[test]
fn the_active_camera_clears_when_it_unmounts() {
    let scene = Scene::new();
    let shown = scene.clone();
    mounted(
        move || {
            render! {
                SceneView(style: Css::new(), scene: shown) {
                    PerspectiveCamera(position: Vec3::new(0.0, 0.0, 3.0))
                }
            }
        },
        |_| {
            let camera: Node = scene.active_camera().expect("camera is active");
            assert_eq!(camera.transform().translation, Vec3::new(0.0, 0.0, 3.0));
        },
    );
    assert!(scene.active_camera().is_none());
}

#[test]
fn an_orthographic_camera_projects_its_height() {
    let scene = Scene::new();
    let shown = scene.clone();
    mounted(
        move || {
            render! {
                SceneView(style: Css::new(), scene: shown) {
                    PerspectiveCamera(active: false)
                    OrthographicCamera(height: 4.0, position: Vec3::new(0.0, 3.0, 0.1))
                }
            }
        },
        |_| {
            let camera: Node = scene.active_camera().expect("camera is active");
            assert_eq!(
                camera.camera(),
                Some(Projection::Orthographic {
                    height: 4.0,
                    near: 0.1,
                    far: 1000.0,
                })
            );
            assert_eq!(camera.transform().translation, Vec3::new(0.0, 3.0, 0.1));
        },
    );
    assert!(scene.active_camera().is_none());
}

#[component]
fn spinner(calls: Rc<Cell<u32>>) -> Element {
    let calls = calls.clone();
    use_frame(move |_| calls.set(calls.get() + 1));
    create_phantom_element()
}

#[test]
fn use_frame_runs_until_unmount() {
    let scene = Scene::new();
    let shown = scene.clone();
    let calls = Rc::new(Cell::new(0));
    let counted = calls.clone();
    let frame = FrameInfo {
        timestamp: Duration::ZERO,
    };
    mounted(
        move || {
            render! {
                SceneView(style: Css::new(), scene: shown) {
                    Spinner(calls: counted.clone())
                }
            }
        },
        |_| {
            scene.inner().run_frame_callbacks(&frame);
            scene.inner().run_frame_callbacks(&frame);
        },
    );
    scene.inner().run_frame_callbacks(&frame);
    assert_eq!(calls.get(), 2);
}

#[test]
fn scene_content_mounts_nodes_without_a_view() {
    let scene = Scene::new();
    let shown = scene.clone();
    mounted(
        move || {
            render! {
                SceneContent(scene: shown) {
                    Group(position: Vec3::X) {
                        Mesh(geometry: cube(), material: red())
                    }
                }
            }
        },
        |_| {
            let top = scene.root().children();
            assert_eq!(top.len(), 1);
            assert_eq!(top[0].transform().translation, Vec3::X);
            assert!(top[0].children()[0].mesh().is_some());
        },
    );
    assert!(scene.root().children().is_empty());
}
