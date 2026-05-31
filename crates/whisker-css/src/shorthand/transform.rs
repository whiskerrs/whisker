//! `transform` — a sequence of transform functions.

use core::fmt;

use crate::data_type::{Angle, Length, LengthPercentage};
use crate::css::Css;
use crate::to_css::{write_number, ToCss};

/// One CSS transform function. Lynx supports the 2-D and 3-D
/// transform families except `rotate3d()` and `scale3d()`.
#[derive(Clone, Debug, PartialEq)]
pub enum TransformFn {
    /// `translate(<x>, <y>)`.
    Translate(LengthPercentage, LengthPercentage),
    /// `translateX(<x>)`.
    TranslateX(LengthPercentage),
    /// `translateY(<y>)`.
    TranslateY(LengthPercentage),
    /// `translateZ(<z>)`.
    TranslateZ(Length),
    /// `translate3d(<x>, <y>, <z>)`.
    Translate3d(LengthPercentage, LengthPercentage, Length),
    /// `rotate(<angle>)` — alias of `rotateZ`.
    Rotate(Angle),
    /// `rotateX(<angle>)`.
    RotateX(Angle),
    /// `rotateY(<angle>)`.
    RotateY(Angle),
    /// `rotateZ(<angle>)`.
    RotateZ(Angle),
    /// `scale(<x>, <y>)`.
    Scale(f32, f32),
    /// `scaleX(<x>)`.
    ScaleX(f32),
    /// `scaleY(<y>)`.
    ScaleY(f32),
    /// `skew(<x-angle>, <y-angle>)`.
    Skew(Angle, Angle),
    /// `skewX(<angle>)`.
    SkewX(Angle),
    /// `skewY(<angle>)`.
    SkewY(Angle),
    /// `matrix(a, b, c, d, tx, ty)`.
    Matrix([f32; 6]),
    /// `matrix3d(...)` — 16-element column-major matrix.
    Matrix3d([f32; 16]),
}

impl ToCss for TransformFn {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            TransformFn::Translate(x, y) => {
                dest.write_str("translate(")?;
                x.to_css(dest)?;
                dest.write_str(", ")?;
                y.to_css(dest)?;
                dest.write_char(')')
            }
            TransformFn::TranslateX(x) => {
                dest.write_str("translateX(")?;
                x.to_css(dest)?;
                dest.write_char(')')
            }
            TransformFn::TranslateY(y) => {
                dest.write_str("translateY(")?;
                y.to_css(dest)?;
                dest.write_char(')')
            }
            TransformFn::TranslateZ(z) => {
                dest.write_str("translateZ(")?;
                z.to_css(dest)?;
                dest.write_char(')')
            }
            TransformFn::Translate3d(x, y, z) => {
                dest.write_str("translate3d(")?;
                x.to_css(dest)?;
                dest.write_str(", ")?;
                y.to_css(dest)?;
                dest.write_str(", ")?;
                z.to_css(dest)?;
                dest.write_char(')')
            }
            TransformFn::Rotate(a) => fn_one(dest, "rotate", a),
            TransformFn::RotateX(a) => fn_one(dest, "rotateX", a),
            TransformFn::RotateY(a) => fn_one(dest, "rotateY", a),
            TransformFn::RotateZ(a) => fn_one(dest, "rotateZ", a),
            TransformFn::Scale(x, y) => {
                dest.write_str("scale(")?;
                write_number(dest, *x)?;
                dest.write_str(", ")?;
                write_number(dest, *y)?;
                dest.write_char(')')
            }
            TransformFn::ScaleX(x) => {
                dest.write_str("scaleX(")?;
                write_number(dest, *x)?;
                dest.write_char(')')
            }
            TransformFn::ScaleY(y) => {
                dest.write_str("scaleY(")?;
                write_number(dest, *y)?;
                dest.write_char(')')
            }
            TransformFn::Skew(x, y) => {
                dest.write_str("skew(")?;
                x.to_css(dest)?;
                dest.write_str(", ")?;
                y.to_css(dest)?;
                dest.write_char(')')
            }
            TransformFn::SkewX(a) => fn_one(dest, "skewX", a),
            TransformFn::SkewY(a) => fn_one(dest, "skewY", a),
            TransformFn::Matrix(m) => {
                dest.write_str("matrix(")?;
                for (i, v) in m.iter().enumerate() {
                    if i > 0 {
                        dest.write_str(", ")?;
                    }
                    write_number(dest, *v)?;
                }
                dest.write_char(')')
            }
            TransformFn::Matrix3d(m) => {
                dest.write_str("matrix3d(")?;
                for (i, v) in m.iter().enumerate() {
                    if i > 0 {
                        dest.write_str(", ")?;
                    }
                    write_number(dest, *v)?;
                }
                dest.write_char(')')
            }
        }
    }
}

fn fn_one(dest: &mut dyn fmt::Write, name: &str, a: &Angle) -> fmt::Result {
    dest.write_str(name)?;
    dest.write_char('(')?;
    a.to_css(dest)?;
    dest.write_char(')')
}

/// A sequence of [`TransformFn`]s — the value of the `transform`
/// property.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transform(pub Vec<TransformFn>);

impl Transform {
    /// An empty transform list (renders as nothing).
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a function.
    pub fn push(mut self, f: TransformFn) -> Self {
        self.0.push(f);
        self
    }
}

impl ToCss for Transform {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        for (i, f) in self.0.iter().enumerate() {
            if i > 0 {
                dest.write_char(' ')?;
            }
            f.to_css(dest)?;
        }
        Ok(())
    }
}

impl From<TransformFn> for Transform {
    fn from(f: TransformFn) -> Self {
        Self(vec![f])
    }
}

impl<const N: usize> From<[TransformFn; N]> for Transform {
    fn from(arr: [TransformFn; N]) -> Self {
        Self(arr.into_iter().collect())
    }
}

impl From<Vec<TransformFn>> for Transform {
    fn from(v: Vec<TransformFn>) -> Self {
        Self(v)
    }
}

impl Css {
    /// Sets `transform` to a list of [`TransformFn`]s. Functions are
    /// applied left-to-right.
    /// <https://lynxjs.org/api/css/properties/transform>
    pub fn transform(self, t: impl Into<Transform>) -> Self {
        self.push("transform", t.into())
    }
}

#[cfg(test)]
mod tests {
    use crate::ext::*;
    use crate::Css;

    use super::*;

    #[test]
    fn single_transform_function() {
        let s = Css::new().transform(TransformFn::TranslateX(px(10).into()));
        assert_eq!(s.to_string(), "transform: translateX(10px);");
    }

    #[test]
    fn chain_of_transforms() {
        let s = Css::new().transform([
            TransformFn::TranslateX(px(10).into()),
            TransformFn::Rotate(45.deg()),
            TransformFn::Scale(1.5, 2.0),
        ]);
        assert_eq!(
            s.to_string(),
            "transform: translateX(10px) rotate(45deg) scale(1.5, 2);"
        );
    }

    #[test]
    fn translate_variants() {
        assert_eq!(
            Css::new()
                .transform(TransformFn::Translate(px(1).into(), px(2).into()))
                .to_string(),
            "transform: translate(1px, 2px);"
        );
        assert_eq!(
            Css::new()
                .transform(TransformFn::TranslateY(px(3).into()))
                .to_string(),
            "transform: translateY(3px);"
        );
        assert_eq!(
            Css::new()
                .transform(TransformFn::TranslateZ(px(4)))
                .to_string(),
            "transform: translateZ(4px);"
        );
        assert_eq!(
            Css::new()
                .transform(TransformFn::Translate3d(
                    px(1).into(),
                    px(2).into(),
                    px(3),
                ))
                .to_string(),
            "transform: translate3d(1px, 2px, 3px);"
        );
    }

    #[test]
    fn rotate_axes() {
        let s = Css::new().transform([
            TransformFn::RotateX(10.deg()),
            TransformFn::RotateY(20.deg()),
            TransformFn::RotateZ(30.deg()),
        ]);
        assert_eq!(
            s.to_string(),
            "transform: rotateX(10deg) rotateY(20deg) rotateZ(30deg);"
        );
    }

    #[test]
    fn scale_axes() {
        let s = Css::new().transform([TransformFn::ScaleX(2.0), TransformFn::ScaleY(0.5)]);
        assert_eq!(s.to_string(), "transform: scaleX(2) scaleY(0.5);");
    }

    #[test]
    fn skew_variants() {
        let s = Css::new().transform([
            TransformFn::Skew(10.deg(), 20.deg()),
            TransformFn::SkewX(5.deg()),
            TransformFn::SkewY(15.deg()),
        ]);
        assert_eq!(
            s.to_string(),
            "transform: skew(10deg, 20deg) skewX(5deg) skewY(15deg);"
        );
    }

    #[test]
    fn matrix_six_values() {
        let s = Css::new().transform(TransformFn::Matrix([1.0, 0.0, 0.0, 1.0, 10.0, 20.0]));
        assert_eq!(s.to_string(), "transform: matrix(1, 0, 0, 1, 10, 20);");
    }

    #[test]
    fn matrix3d_sixteen_values() {
        let mut m = [0.0_f32; 16];
        m[0] = 1.0;
        m[5] = 1.0;
        m[10] = 1.0;
        m[15] = 1.0;
        let s = Css::new().transform(TransformFn::Matrix3d(m));
        assert_eq!(
            s.to_string(),
            "transform: matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1);"
        );
    }

    #[test]
    fn transform_from_vec_and_array() {
        let from_vec = Css::new().transform(vec![TransformFn::Rotate(10.deg())]);
        let from_arr = Css::new().transform([TransformFn::Rotate(10.deg())]);
        assert_eq!(from_vec.to_string(), from_arr.to_string());
    }

    #[test]
    fn transform_builder_pattern() {
        let t = Transform::new()
            .push(TransformFn::TranslateX(px(10).into()))
            .push(TransformFn::Rotate(45.deg()));
        let s = Css::new().transform(t);
        assert_eq!(
            s.to_string(),
            "transform: translateX(10px) rotate(45deg);"
        );
    }
}
