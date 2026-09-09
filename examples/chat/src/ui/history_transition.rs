use whisker::AnimConfig;
use whisker::css::{Transform, TransformFn, ext::percent};
use whisker_router::{Pose, PoseContext, Role, RouteTransition, Transition};

pub fn drawer_transition() -> RouteTransition {
    RouteTransition::custom(HistoryDrawer)
}

struct HistoryDrawer;

impl Transition for HistoryDrawer {
    fn config(&self) -> AnimConfig {
        AnimConfig::ease_out(240)
    }

    fn pose(&self, context: PoseContext) -> Pose {
        match context.role {
            Role::Top => Pose::new(
                Transform::new().push(TransformFn::TranslateX(
                    percent(-100.0 * (1.0 - context.progress)).into(),
                )),
                1.0,
            ),
            Role::Under => Pose::new(Transform::new(), 1.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_router::Direction;

    #[test]
    fn history_enters_and_leaves_on_the_left_without_moving_chat() {
        let drawer = drawer_transition();
        for direction in [Direction::Push, Direction::Pop] {
            let hidden = drawer.pose(PoseContext::new(Role::Top, 0.0, direction));
            let shown = drawer.pose(PoseContext::new(Role::Top, 1.0, direction));
            assert_eq!(
                hidden.transform,
                Transform::new().push(TransformFn::TranslateX(percent(-100).into()))
            );
            assert_eq!(
                shown.transform,
                Transform::new().push(TransformFn::TranslateX(percent(0).into()))
            );
            for progress in [0.0, 0.5, 1.0] {
                assert_eq!(
                    drawer.pose(PoseContext::new(Role::Under, progress, direction)),
                    Pose::new(Transform::new(), 1.0)
                );
            }
        }
    }
}
