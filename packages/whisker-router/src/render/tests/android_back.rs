use super::*;
use crate::render::handle::StackBridge;
use crate::render::platform_navigation::begin_with_mode;
use crate::render::transition::{Direction, PoseMode, pose_for};
use whisker::css::{LengthPercentage, TransformFn};

fn android_stack() -> RouterHandle {
    let tree = CompiledTree::new(RouteTree::Stack(vec![
        RouteTree::route("", "home"),
        RouteTree::route("detail/:id", "detail"),
    ]));
    let mut registry = RouteRegistry::new();
    for id in ["home", "detail"] {
        registry = registry.route_with(id, RouteTransition::slide_fade(), |_: &RouteInstance| {
            whisker::runtime::view::create_phantom_element()
        });
    }
    let nav = RouterHandle::new((tree, registry));
    mount_node(&nav, NodePath::root());
    flush();
    nav.navigate("/detail/1").unwrap();
    flush();
    settle_animations();
    nav
}

fn assert_button_slide_fade(bridge: &StackBridge) {
    for (binding, translation, opacity) in [
        (bridge.top_pose.as_ref().unwrap(), 4.0, 0.5),
        (bridge.under_pose.as_ref().unwrap(), -2.0, 1.0),
    ] {
        let mode = binding.mode.get_untracked();
        assert!(matches!(mode, PoseMode::Transition(_, Direction::Pop)));
        let pose = pose_for(&mode, binding.role.get_untracked(), 0.5);
        let [TransformFn::TranslateX(LengthPercentage::Percentage(x))] =
            pose.transform.0.as_slice()
        else {
            panic!("button back must only translate horizontally: {pose:?}");
        };
        assert!((x.0 - translation).abs() < 0.0001);
        assert_eq!(pose.opacity, opacity);
        assert_eq!(pose.radius_px, 0.0);
    }
    assert!(bridge.dim_drive.unwrap().get_untracked().is_none());
}

#[test]
fn android_button_back_uses_a_small_slide_and_fade_without_scale() {
    with_runtime(|| {
        let nav = android_stack();
        let bridge = nav.active_stack_bridge().unwrap();
        nav.back().unwrap();
        flush();
        assert_button_slide_fade(&bridge);
        settle_animations();
        assert_eq!(nav.current().get().path, NodePath(vec![0]));
    });
}

#[test]
fn android_button_preview_events_do_not_start_a_swipe() {
    use crate::render::platform_navigation::install_android_predictive_back;
    use whisker::platform_module::WhiskerValue;
    use whisker::runtime::module::{ModuleHost, with_module_host};

    for with_progress in [false, true] {
        with_runtime(|| {
            let host = ModuleHost::new(
                |_, _, _, _, result| {
                    result(WhiskerValue::Null);
                    true
                },
                |_, _, _| {},
            );
            with_module_host(&host, || {
                let nav = android_stack();
                let bridge = nav.active_stack_bridge().unwrap();
                install_android_predictive_back(nav.clone());
                let payload = WhiskerValue::Map(
                    [
                        ("swipeEdge".into(), WhiskerValue::Int(2)),
                        ("progress".into(), WhiskerValue::Float(0.5)),
                    ]
                    .into_iter()
                    .collect(),
                );
                let events = if with_progress {
                    &["backStarted", "backProgressed"][..]
                } else {
                    &["backStarted"][..]
                };
                for event in events {
                    assert!(host.dispatch_event(
                        "whisker-router:PredictiveBack",
                        event,
                        payload.clone()
                    ));
                }
                flush();
                let binding = bridge.top_pose.as_ref().unwrap();
                assert!(matches!(
                    binding.mode.get_untracked(),
                    PoseMode::Transition(_, Direction::Push)
                ));
                assert_eq!(
                    binding.ctrl.get_untracked().value().get_untracked(),
                    1.0,
                    "a button preview must not scrub the swipe controller"
                );
                assert!(host.dispatch_event(
                    "whisker-router:PredictiveBack",
                    "backInvoked",
                    WhiskerValue::Null
                ));
                flush();
                assert_button_slide_fade(&bridge);
                settle_animations();
                assert_eq!(nav.current().get().path, NodePath(vec![0]));
            });
        });
    }
}

#[test]
fn android_swipe_back_uses_predictive_preview_on_both_edges() {
    for edge in [SwipeEdge::Left, SwipeEdge::Right] {
        with_runtime(|| {
            let nav = android_stack();
            let bridge = begin_with_mode(&nav, PoseMode::Predictive(edge)).unwrap();
            scrub(&bridge, 0.5);
            flush();
            for binding in [bridge.top_pose.as_ref(), bridge.under_pose.as_ref()] {
                let binding = binding.unwrap();
                let mode = binding.mode.get_untracked();
                assert!(matches!(mode, PoseMode::Predictive(actual) if actual == edge));
                let progress = binding.ctrl.get_untracked().value().get_untracked();
                let pose = pose_for(&mode, binding.role.get_untracked(), progress);
                assert!(pose.transform.to_css_string().contains("scale(0.9, 0.9)"));
                assert!(pose.radius_px > 0.0);
            }
            assert!(bridge.dim_drive.unwrap().get_untracked().is_some());
            assert_eq!(nav.current().get().path, NodePath(vec![1]));
            settle(&nav, &bridge, true, None);
            settle_animations();
            assert_eq!(nav.current().get().path, NodePath(vec![0]));
        });
    }
}

#[test]
fn android_button_back_after_cancelled_swipe_does_not_reuse_predictive_pose() {
    with_runtime(|| {
        let nav = android_stack();
        let bridge = begin_with_mode(&nav, PoseMode::Predictive(SwipeEdge::Left)).unwrap();
        scrub(&bridge, 0.35);
        settle(&nav, &bridge, false, None);
        settle_animations();
        assert_eq!(nav.current().get().path, NodePath(vec![1]));
        nav.back().unwrap();
        flush();
        assert_button_slide_fade(&bridge);
        settle_animations();
        assert_eq!(nav.current().get().path, NodePath(vec![0]));
    });
}
