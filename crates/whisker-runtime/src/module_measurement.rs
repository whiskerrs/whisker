//! Property-driven intrinsic measurement for module elements.

use std::collections::BTreeMap;
pub use whisker_protocol::{
    CustomMeasurePayload, MeasureFontFamily, MeasureFontStyle, MeasureLineHeight,
};
use whisker_protocol::{ElementRegistration, PropertyId, TextStyleSnapshot, WhiskerValue};

/// Current inputs available when a module builds its Host measurement payload.
/// The Runtime calls the declared payload function before layout, after reactive property updates.
pub struct ModuleMeasureContext<'a> {
    pub(crate) registration: &'a ElementRegistration,
    pub(crate) properties: &'a BTreeMap<PropertyId, WhiskerValue>,
    pub(crate) scale_factor: f32,
    pub(crate) text_style: Option<&'a TextStyleSnapshot>,
}

impl ModuleMeasureContext<'_> {
    /// Physical pixels per logical layout unit for this surface.
    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// Returns a currently set property by its declared Host name.
    pub fn property(&self, name: &str) -> Option<&WhiskerValue> {
        let property = self.registration.property_named(name)?;
        self.properties.get(&property.property)
    }

    /// Resolved inherited text style, when the element declares `text_style = true`.
    pub fn text_style(&self) -> Option<&TextStyleSnapshot> {
        self.text_style
    }
}

/// Builds versioned Host inputs without accessing Signals or mounted native views.
/// Include every size-affecting input in the payload; the Runtime hashes its bytes.
/// Returning `None` disables intrinsic measurement for the element.
pub type MeasurementPayloadBuilder = fn(ModuleMeasureContext<'_>) -> Option<CustomMeasurePayload>;
