//! Typed non-CSS options for built-in controls.

/// Item-aligned scroll settling configuration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollSnap {
    factor: f64,
    offset: f64,
}

impl ScrollSnap {
    /// Creates item snapping at a normalized anchor (`0` start, `1` end).
    pub fn item(factor: f64, offset: f64) -> Self {
        assert!(factor.is_finite() && (0.0..=1.0).contains(&factor));
        assert!(offset.is_finite());
        Self { factor, offset }
    }

    /// Aligns the start edge of each item with the viewport start edge.
    pub const fn start() -> Self {
        Self {
            factor: 0.0,
            offset: 0.0,
        }
    }

    /// Aligns the center of each item with the viewport center.
    pub const fn center() -> Self {
        Self {
            factor: 0.5,
            offset: 0.0,
        }
    }

    /// Aligns the end edge of each item with the viewport end edge.
    pub const fn end() -> Self {
        Self {
            factor: 1.0,
            offset: 0.0,
        }
    }

    /// Applies a logical-pixel displacement to the selected anchor.
    pub fn with_offset(mut self, offset: f64) -> Self {
        assert!(offset.is_finite());
        self.offset = offset;
        self
    }

    pub(crate) const fn factor(self) -> f64 {
        self.factor
    }

    pub(crate) const fn offset(self) -> f64 {
        self.offset
    }
}

/// Whether one scroll gesture may pass intermediate snap points.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ScrollSnapStop {
    /// Native momentum may settle on any later snap point.
    Normal,
    /// One gesture may advance by at most one snap point.
    Always,
}

impl ScrollSnapStop {
    /// Stable value passed to the built-in ScrollView module.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Always => "always",
        }
    }
}

impl std::fmt::Display for ScrollSnapStop {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_snap_stop_has_stable_values() {
        assert_eq!(ScrollSnapStop::Normal.as_str(), "normal");
        assert_eq!(ScrollSnapStop::Always.as_str(), "always");
    }

    #[test]
    fn scroll_snap_presets_are_normalized() {
        assert_eq!(ScrollSnap::start().factor(), 0.0);
        assert_eq!(ScrollSnap::center().factor(), 0.5);
        assert_eq!(ScrollSnap::end().factor(), 1.0);
    }
}
