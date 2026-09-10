use super::*;
use crate::render::handle::StackBridge;
use crate::render::platform_navigation::begin_with_mode;
use crate::render::transition::{
    AndroidDefault, Direction, PoseContext, PoseMode, Role, Transition, pose_for,
};
use whisker::css::{LengthPercentage, TransformFn};

fn android_stack() -> RouterHandle {
    let tree = CompiledTree::new(RouteTree::Stack(vec![
        RouteTree::route("", "home"),
        RouteTree::route("detail/:id", "detail"),
    ]));
    let mut registry = RouteRegistry::new();
    for id in ["home", "detail"] {
        registry = registry.route_with(
            id,
            RouteTransition::custom(AndroidDefault),
            |_: &RouteInstance| whisker::runtime::view::create_phantom_element(),
        );
    }
    let nav = RouterHandle::new((tree, registry));
    mount_node(&nav, NodePath::root());
    flush();
    nav.navigate("/detail/1").unwrap();
    flush();
    settle_animations();
    nav
}

fn assert_button_slide(bridge: &StackBridge) {
    for (binding, translation) in [
        (bridge.top_pose.as_ref().unwrap(), 50.0),
        (bridge.under_pose.as_ref().unwrap(), -15.0),
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
        assert_eq!(pose.opacity, 1.0);
        assert_eq!(pose.radius_px, 0.0);
    }
    assert!(bridge.dim_drive.unwrap().get_untracked().is_none());
}

#[test]
fn android_button_back_slides_both_routes_without_scale_or_fade() {
    with_runtime(|| {
        let nav = android_stack();
        let bridge = nav.active_stack_bridge().unwrap();
        nav.back().unwrap();
        flush();
        assert_button_slide(&bridge);
        settle_animations();
        assert_eq!(nav.current().get().path, NodePath(vec![0]));
    });
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
        assert_button_slide(&bridge);
        settle_animations();
        assert_eq!(nav.current().get().path, NodePath(vec![0]));
    });
}

#[test]
fn android_push_keeps_the_slide_fade_transition() {
    for role in [Role::Top, Role::Under] {
        for progress in [0.0, 0.5, 1.0] {
            let ctx = PoseContext::new(role, progress, Direction::Push);
            let actual = AndroidDefault.pose(ctx);
            let expected = RouteTransition::slide_fade().pose(ctx);
            assert_eq!(actual.transform, expected.transform);
            assert_eq!(actual.opacity, expected.opacity);
        }
    }
}
