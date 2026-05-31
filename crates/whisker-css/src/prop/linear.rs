//! Lynx-only `linear-*` layout extensions. Used when
//! `display: linear` (Lynx's default for `<view>`).

use crate::keyword::{
    LinearCrossGravity, LinearGravity, LinearLayoutGravity, LinearOrientation,
};
use crate::css::Css;

impl Css {
    /// Sets `linear-orientation` — Lynx's analogue of `flex-direction`.
    /// <https://lynxjs.org/api/css/properties/linear-orientation>
    pub fn linear_orientation(self, v: LinearOrientation) -> Self {
        self.push("linear-orientation", v)
    }

    /// Sets `linear-direction` — direction the linear container flows.
    /// <https://lynxjs.org/api/css/properties/linear-direction>
    pub fn linear_direction(self, v: LinearOrientation) -> Self {
        self.push("linear-direction", v)
    }

    /// Sets `linear-gravity` — main-axis alignment. **Deprecated**;
    /// switch to `display: flex` + `justify-content` when possible.
    /// <https://lynxjs.org/api/css/properties/linear-gravity>
    pub fn linear_gravity(self, v: LinearGravity) -> Self {
        self.push("linear-gravity", v)
    }

    /// Sets `linear-cross-gravity` — cross-axis alignment for all items.
    /// <https://lynxjs.org/api/css/properties/linear-cross-gravity>
    pub fn linear_cross_gravity(self, v: LinearCrossGravity) -> Self {
        self.push("linear-cross-gravity", v)
    }

    /// Sets `linear-layout-gravity` — per-item cross-axis override.
    /// <https://lynxjs.org/api/css/properties/linear-layout-gravity>
    pub fn linear_layout_gravity(self, v: LinearLayoutGravity) -> Self {
        self.push("linear-layout-gravity", v)
    }

    /// Sets `linear-weight` — relative size weight along the main axis.
    /// <https://lynxjs.org/api/css/properties/linear-weight>
    pub fn linear_weight(self, v: f32) -> Self {
        self.push_raw("linear-weight", crate::to_css::number_to_string(v))
    }

    /// Sets `linear-weight-sum` — denominator for weight calculations.
    /// <https://lynxjs.org/api/css/properties/linear-weight-sum>
    pub fn linear_weight_sum(self, v: f32) -> Self {
        self.push_raw("linear-weight-sum", crate::to_css::number_to_string(v))
    }
}

#[cfg(test)]
mod tests {
    use crate::keyword::*;
    use crate::Css;

    #[test]
    fn linear_orientation_and_direction() {
        let s = Css::new()
            .linear_orientation(LinearOrientation::Vertical)
            .linear_direction(LinearOrientation::Horizontal);
        assert_eq!(
            s.to_string(),
            "linear-orientation: vertical; linear-direction: horizontal;"
        );
    }

    #[test]
    fn linear_gravity_all_three() {
        let s = Css::new()
            .linear_gravity(LinearGravity::CenterVertical)
            .linear_cross_gravity(LinearCrossGravity::Center)
            .linear_layout_gravity(LinearLayoutGravity::Stretch);
        assert_eq!(
            s.to_string(),
            "linear-gravity: center-vertical; linear-cross-gravity: center; linear-layout-gravity: stretch;"
        );
    }

    #[test]
    fn linear_weights() {
        let s = Css::new().linear_weight(1.0).linear_weight_sum(3.0);
        assert_eq!(
            s.to_string(),
            "linear-weight: 1; linear-weight-sum: 3;"
        );
    }
}
