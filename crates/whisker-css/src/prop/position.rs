//! Position properties: `position`, edges (`top`/`right`/`bottom`/
//! `left`), `z-index`, `inset*`.

use crate::data_type::LengthPercentage;
use crate::data_type_ext::Integer;
use crate::keyword::PositionKind;
use crate::css::Css;

impl Css {
    /// Sets `position`. Lynx default: `relative`.
    /// `static` is **not** supported by Lynx.
    /// <https://lynxjs.org/api/css/properties/position>
    pub fn position(self, v: PositionKind) -> Self {
        self.push("position", v)
    }

    /// Sets `top` offset (positioned elements).
    /// <https://lynxjs.org/api/css/properties/top>
    pub fn top(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("top", v.into())
    }

    /// Sets `right` offset (positioned elements).
    /// <https://lynxjs.org/api/css/properties/right>
    pub fn right(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("right", v.into())
    }

    /// Sets `bottom` offset (positioned elements).
    /// <https://lynxjs.org/api/css/properties/bottom>
    pub fn bottom(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("bottom", v.into())
    }

    /// Sets `left` offset (positioned elements).
    /// <https://lynxjs.org/api/css/properties/left>
    pub fn left(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("left", v.into())
    }

    /// Sets `inset-inline-start` — logical start edge.
    /// <https://lynxjs.org/api/css/properties/inset-inline-start>
    pub fn inset_inline_start(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("inset-inline-start", v.into())
    }

    /// Sets `inset-inline-end` — logical end edge.
    /// <https://lynxjs.org/api/css/properties/inset-inline-end>
    pub fn inset_inline_end(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("inset-inline-end", v.into())
    }

    /// Sets `z-index`. Lynx default: `auto` (no stacking context promotion).
    /// <https://lynxjs.org/api/css/properties/z-index>
    pub fn z_index(self, v: i32) -> Self {
        self.push("z-index", Integer(v))
    }
}

#[cfg(test)]
mod tests {
    use crate::ext::*;
    use crate::keyword::PositionKind;
    use crate::Css;

    #[test]
    fn position_absolute_with_offsets() {
        let s = Css::new()
            .position(PositionKind::Absolute)
            .top(px(10))
            .right(0.percent())
            .bottom(px(10))
            .left(0.percent());
        assert_eq!(
            s.to_string(),
            "position: absolute; top: 10px; right: 0%; bottom: 10px; left: 0%;"
        );
    }

    #[test]
    fn z_index_negative_allowed() {
        let s = Css::new().z_index(-1);
        assert_eq!(s.to_string(), "z-index: -1;");
    }

    #[test]
    fn inset_inline_logical_edges() {
        let s = Css::new()
            .inset_inline_start(px(4))
            .inset_inline_end(px(8));
        assert_eq!(
            s.to_string(),
            "inset-inline-start: 4px; inset-inline-end: 8px;"
        );
    }

    #[test]
    fn all_position_keywords() {
        let cases = [
            (PositionKind::Relative, "relative"),
            (PositionKind::Absolute, "absolute"),
            (PositionKind::Fixed, "fixed"),
            (PositionKind::Sticky, "sticky"),
        ];
        for (k, expected) in cases {
            let s = Css::new().position(k);
            assert_eq!(s.to_string(), format!("position: {expected};"));
        }
    }
}
