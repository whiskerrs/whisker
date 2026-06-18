//! Flexbox properties.

use crate::css::Css;
use crate::keyword::{
    AlignContent, AlignItems, AlignSelf, FlexDirection, FlexWrap, JustifyContent,
};
use crate::value::FlexBasis;

impl Css {
    /// Sets `flex-direction`. Lynx default: `row`.
    /// <https://lynxjs.org/api/css/properties/flex-direction>
    pub fn flex_direction(self, v: FlexDirection) -> Self {
        self.push("flex-direction", v)
    }

    /// Sets `flex-wrap`. Lynx default: `nowrap`.
    /// <https://lynxjs.org/api/css/properties/flex-wrap>
    pub fn flex_wrap(self, v: FlexWrap) -> Self {
        self.push("flex-wrap", v)
    }

    /// Sets `flex-grow`. Lynx default: `0`. Negative values are
    /// clamped to zero.
    /// <https://lynxjs.org/api/css/properties/flex-grow>
    pub fn flex_grow(self, v: f32) -> Self {
        self.push_raw("flex-grow", crate::to_css::number_to_string(v))
    }

    /// Sets `flex-shrink`. Lynx default: `1`. Negative values are
    /// clamped to zero.
    /// <https://lynxjs.org/api/css/properties/flex-shrink>
    pub fn flex_shrink(self, v: f32) -> Self {
        self.push_raw("flex-shrink", crate::to_css::number_to_string(v))
    }

    /// Sets `flex-basis`. Lynx default: `auto`.
    /// <https://lynxjs.org/api/css/properties/flex-basis>
    pub fn flex_basis(self, v: impl Into<FlexBasis>) -> Self {
        self.push("flex-basis", v.into())
    }

    /// Sets `justify-content` — main-axis distribution.
    /// <https://lynxjs.org/api/css/properties/justify-content>
    pub fn justify_content(self, v: JustifyContent) -> Self {
        self.push("justify-content", v)
    }

    /// Sets `align-items` — cross-axis alignment for all items.
    /// <https://lynxjs.org/api/css/properties/align-items>
    pub fn align_items(self, v: AlignItems) -> Self {
        self.push("align-items", v)
    }

    /// Sets `align-self` — cross-axis alignment for this item only.
    /// <https://lynxjs.org/api/css/properties/align-self>
    pub fn align_self(self, v: AlignSelf) -> Self {
        self.push("align-self", v)
    }

    /// Sets `align-content` — cross-axis distribution of wrapped lines.
    /// <https://lynxjs.org/api/css/properties/align-content>
    pub fn align_content(self, v: AlignContent) -> Self {
        self.push("align-content", v)
    }

    /// Sets `order` — controls layout order among flex/grid siblings.
    /// <https://lynxjs.org/api/css/properties/order>
    pub fn order(self, v: i32) -> Self {
        self.push_raw("order", v.to_string())
    }
}

#[cfg(test)]
mod tests {
    use crate::Css;
    use crate::ext::*;
    use crate::keyword::*;
    use crate::value::FlexBasis;

    #[test]
    fn flex_direction_and_wrap() {
        let s = Css::new()
            .flex_direction(FlexDirection::Column)
            .flex_wrap(FlexWrap::Wrap);
        assert_eq!(s.to_string(), "flex-direction: column; flex-wrap: wrap;");
    }

    #[test]
    fn flex_grow_shrink_basis() {
        let s = Css::new()
            .flex_grow(1.0)
            .flex_shrink(0.5)
            .flex_basis(FlexBasis::Auto);
        assert_eq!(
            s.to_string(),
            "flex-grow: 1; flex-shrink: 0.5; flex-basis: auto;"
        );
    }

    #[test]
    fn flex_basis_length_and_content() {
        let s = Css::new().flex_basis(px(100));
        assert_eq!(s.to_string(), "flex-basis: 100px;");
        let s = Css::new().flex_basis(FlexBasis::Content);
        assert_eq!(s.to_string(), "flex-basis: content;");
    }

    #[test]
    fn alignment_set() {
        let s = Css::new()
            .justify_content(JustifyContent::SpaceBetween)
            .align_items(AlignItems::Center)
            .align_self(AlignSelf::Stretch)
            .align_content(AlignContent::SpaceAround);
        assert_eq!(
            s.to_string(),
            "justify-content: space-between; align-items: center; align-self: stretch; align-content: space-around;"
        );
    }

    #[test]
    fn order_negative_allowed() {
        let s = Css::new().order(-1);
        assert_eq!(s.to_string(), "order: -1;");
    }
}
