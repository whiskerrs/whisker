//! Pure replay of Lynx's capture→bubble→catch event-propagation
//! ordering, decoupled from the bridge so it's unit-testable.
//!
//! Whisker reconstructs propagation in Rust because Lynx's reporter
//! hook fires once at the hit-tested target, *before* — and bypassing
//! — the engine's own capture/bubble chain (whose per-element firings
//! go to the absent JS runtime). This module is the faithful Rust
//! port of `TouchEventHandler::HandleEventInternal`
//! (`core/renderer/events/touch_event_handler.cc`).

use whisker_runtime::view::BindType;

/// Plan the firing order for an event over `chain` — the response
/// chain, **target-first**: `[target, parent, …, root]`.
/// `handlers_for(sign)` yields that element's `(BindType, listener)`
/// entries (in registration order); return an empty slice for an
/// element with no listener for this event.
///
/// Mirrors Lynx's `HandleEventInternal`:
///   - **capture phase** (root → target): every `CaptureBind` fires;
///     a `CaptureCatch` fires too but then stops *everything* — no
///     bubble phase runs at all.
///   - **bubble phase** (target → root), only reached if no
///     capture-catch fired: every `Bind` fires; a `Catch` fires then
///     stops further bubbling.
///
/// Within a single element all its handlers are visited before the
/// stop takes effect (matching Lynx's inner loop + `need_break`), so
/// an element carrying both a capture-bind and a capture-catch fires
/// both before propagation halts.
///
/// Returns `(consumed, firings)`: `firings` is `(sign, listener)` in
/// fire order, and `consumed` is whether any listener matched (relayed
/// to the platform reporter so Lynx skips its native chain).
pub(super) fn plan<'a, T, F>(chain: &[i32], handlers_for: F) -> (bool, Vec<(i32, T)>)
where
    T: Clone + 'a,
    F: Fn(i32) -> &'a [(BindType, T)],
{
    let mut firings: Vec<(i32, T)> = Vec::new();
    let mut consumed = false;
    let mut capture_caught = false;

    // Capture phase: root → target (chain is target-first, so reverse).
    for &sign in chain.iter().rev() {
        let mut stop = false;
        for (bt, listener) in handlers_for(sign) {
            match bt {
                BindType::CaptureCatch => {
                    firings.push((sign, listener.clone()));
                    consumed = true;
                    capture_caught = true;
                    stop = true;
                }
                BindType::CaptureBind => {
                    firings.push((sign, listener.clone()));
                    consumed = true;
                }
                _ => {}
            }
        }
        if stop {
            break;
        }
    }

    // Bubble phase: target → root, skipped entirely if a capture-catch
    // already swallowed the event.
    if !capture_caught {
        for &sign in chain.iter() {
            let mut stop = false;
            for (bt, listener) in handlers_for(sign) {
                match bt {
                    BindType::Catch => {
                        firings.push((sign, listener.clone()));
                        consumed = true;
                        stop = true;
                    }
                    BindType::Bind => {
                        firings.push((sign, listener.clone()));
                        consumed = true;
                    }
                    _ => {}
                }
            }
            if stop {
                break;
            }
        }
    }

    (consumed, firings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Build a `handlers_for` closure from a sign → handlers map.
    fn lookup<'a>(
        map: &'a HashMap<i32, Vec<(BindType, &'static str)>>,
    ) -> impl Fn(i32) -> &'a [(BindType, &'static str)] + 'a {
        move |sign| map.get(&sign).map(Vec::as_slice).unwrap_or(&[])
    }

    // Chain target-first: target=1, parent=2, root=3  (tree 3 → 2 → 1).
    const TARGET: i32 = 1;
    const PARENT: i32 = 2;
    const ROOT: i32 = 3;
    fn chain() -> Vec<i32> {
        vec![TARGET, PARENT, ROOT]
    }

    fn order(firings: &[(i32, &'static str)]) -> Vec<&'static str> {
        firings.iter().map(|(_, l)| *l).collect()
    }

    #[test]
    fn bubble_runs_target_to_root() {
        let mut m = HashMap::new();
        m.insert(TARGET, vec![(BindType::Bind, "target")]);
        m.insert(PARENT, vec![(BindType::Bind, "parent")]);
        m.insert(ROOT, vec![(BindType::Bind, "root")]);
        let (consumed, firings) = plan(&chain(), lookup(&m));
        assert!(consumed);
        assert_eq!(order(&firings), ["target", "parent", "root"]);
    }

    #[test]
    fn catch_stops_bubbling_at_that_element() {
        let mut m = HashMap::new();
        m.insert(TARGET, vec![(BindType::Bind, "target")]);
        m.insert(PARENT, vec![(BindType::Catch, "parent_catch")]);
        m.insert(ROOT, vec![(BindType::Bind, "root")]);
        let (_, firings) = plan(&chain(), lookup(&m));
        // target fires, parent's catch fires + stops — root never runs.
        assert_eq!(order(&firings), ["target", "parent_catch"]);
    }

    #[test]
    fn capture_runs_root_to_target_before_bubble() {
        let mut m = HashMap::new();
        m.insert(ROOT, vec![(BindType::CaptureBind, "root_cap")]);
        m.insert(PARENT, vec![(BindType::CaptureBind, "parent_cap")]);
        m.insert(TARGET, vec![(BindType::Bind, "target_bubble")]);
        let (_, firings) = plan(&chain(), lookup(&m));
        // capture root→target, then bubble target→root.
        assert_eq!(order(&firings), ["root_cap", "parent_cap", "target_bubble"]);
    }

    #[test]
    fn capture_catch_stops_everything_including_bubble() {
        let mut m = HashMap::new();
        m.insert(ROOT, vec![(BindType::CaptureCatch, "root_cap_catch")]);
        m.insert(PARENT, vec![(BindType::CaptureBind, "parent_cap")]);
        m.insert(TARGET, vec![(BindType::Bind, "target_bubble")]);
        let (consumed, firings) = plan(&chain(), lookup(&m));
        assert!(consumed);
        // root's capture-catch fires and swallows the event: no
        // descendant capture, no bubble.
        assert_eq!(order(&firings), ["root_cap_catch"]);
    }

    #[test]
    fn capture_bind_does_not_stop_bubble() {
        let mut m = HashMap::new();
        m.insert(PARENT, vec![(BindType::CaptureBind, "parent_cap")]);
        m.insert(TARGET, vec![(BindType::Bind, "target_bubble")]);
        let (_, firings) = plan(&chain(), lookup(&m));
        assert_eq!(order(&firings), ["parent_cap", "target_bubble"]);
    }

    #[test]
    fn element_with_capture_and_bubble_both_fire() {
        let mut m = HashMap::new();
        // One element registers both phases.
        m.insert(
            TARGET,
            vec![
                (BindType::CaptureBind, "t_cap"),
                (BindType::Bind, "t_bubble"),
            ],
        );
        let (_, firings) = plan(&chain(), lookup(&m));
        // capture pass fires t_cap, bubble pass fires t_bubble.
        assert_eq!(order(&firings), ["t_cap", "t_bubble"]);
    }

    #[test]
    fn no_handlers_not_consumed() {
        let m: HashMap<i32, Vec<(BindType, &'static str)>> = HashMap::new();
        let (consumed, firings) = plan(&chain(), lookup(&m));
        assert!(!consumed);
        assert!(firings.is_empty());
    }
}
