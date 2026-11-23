use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use glam::{Quat, Vec3};
use whisker_surface::FrameInfo;

use super::*;

#[test]
fn add_child_moves_a_node_between_parents() {
    let (a, b, child) = (Node::new(), Node::new(), Node::new());
    a.add_child(&child);
    b.add_child(&child);
    assert!(a.children().is_empty());
    assert_eq!(b.children(), [child.clone()]);
    assert_eq!(child.parent(), Some(b.clone()));
    child.remove_from_parent();
    assert!(b.children().is_empty());
    assert_eq!(child.parent(), None);
}

#[test]
fn insert_child_clamps_its_index() {
    let (parent, first, second, third) = (Node::new(), Node::new(), Node::new(), Node::new());
    parent.add_child(&first);
    parent.insert_child(0, &second);
    parent.insert_child(9, &third);
    assert_eq!(parent.children(), [second, first, third]);
}

#[test]
#[should_panic(expected = "own descendant")]
fn a_node_cannot_contain_its_ancestor() {
    let (parent, child) = (Node::new(), Node::new());
    parent.add_child(&child);
    child.add_child(&parent);
}

#[test]
fn looking_at_points_negative_z_at_the_target() {
    let eye = Vec3::new(1.0, 2.0, 3.0);
    let target = Vec3::new(-2.0, 0.5, 0.0);
    let transform = Transform::looking_at(eye, target, Vec3::Y);
    let forward = transform.rotation * Vec3::NEG_Z;
    assert!(forward.abs_diff_eq((target - eye).normalize(), 1e-5));
    assert!((transform.rotation * Vec3::Y).y > 0.0);
}

#[test]
fn rotation_toward_handles_straight_up_and_zero() {
    let down = rotation_toward(Vec3::NEG_Y, Vec3::Y);
    assert!((down * Vec3::NEG_Z).abs_diff_eq(Vec3::NEG_Y, 1e-5));
    assert_eq!(rotation_toward(Vec3::ZERO, Vec3::Y), Quat::IDENTITY);
}

#[test]
fn dropping_a_frame_subscription_unregisters_it() {
    let scene = Scene::new();
    let calls = Rc::new(Cell::new(0));
    let counted = calls.clone();
    let subscription = scene.on_frame(move |_| counted.set(counted.get() + 1));
    let frame = FrameInfo {
        timestamp: Duration::ZERO,
    };
    scene.inner().run_frame_callbacks(&frame);
    drop(subscription);
    scene.inner().run_frame_callbacks(&frame);
    assert_eq!(calls.get(), 1);
}

#[test]
fn a_frame_callback_may_subscribe_another() {
    let scene = Scene::new();
    let inner_scene = scene.clone();
    let held = Rc::new(std::cell::RefCell::new(Vec::new()));
    let keep = held.clone();
    let _outer = scene.on_frame(move |_| keep.borrow_mut().push(inner_scene.on_frame(|_| {})));
    scene.inner().run_frame_callbacks(&FrameInfo {
        timestamp: Duration::ZERO,
    });
    assert_eq!(held.borrow().len(), 1);
}
