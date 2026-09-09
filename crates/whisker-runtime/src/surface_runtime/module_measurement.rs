use super::*;
use crate::module_measurement::{MeasurementPayloadBuilder, ModuleMeasureContext};
use std::hash::{DefaultHasher, Hash, Hasher};
use whisker_protocol::{
    MeasurementPayload, MeasurementSpec, PendingMeasurePolicy, PropertyId, TextStyleSnapshot,
};

#[derive(PartialEq)]
struct Inputs {
    scale_factor: f32,
    properties: BTreeMap<PropertyId, WhiskerValue>,
    text_style: Option<TextStyleSnapshot>,
}

pub(super) struct Binding {
    builder: MeasurementPayloadBuilder,
    previous: Option<Inputs>,
}

impl BindingState {
    pub(super) fn bind_measurement(
        &mut self,
        element: Element,
        builder: MeasurementPayloadBuilder,
    ) -> Result<(), RuntimeBindingError> {
        let entry = self.element(element)?;
        if !entry.kind.registration().is_some_and(|registration| {
            registration.measurement == whisker_protocol::ElementMeasurement::Custom
                && registration.child_policy == whisker_protocol::ChildPolicy::None
        }) {
            return Err(RuntimeBindingError::InvalidMeasurementBinding { element });
        }
        self.module_measurements.insert(
            element,
            Binding {
                builder,
                previous: None,
            },
        );
        Ok(())
    }

    pub(super) fn flush_module_measurements(&mut self) -> Result<(), RuntimeBindingError> {
        for (element, binding) in &mut self.module_measurements {
            let Some(entry) = self.elements.get(element) else {
                continue;
            };
            let Some(node) = entry.node else { continue };
            let Some(scene) = self.surface.node(node) else {
                continue;
            };
            if binding.previous.as_ref().is_some_and(|previous| {
                previous.scale_factor == self.environment.scale_factor()
                    && &previous.properties == scene.properties()
                    && previous.text_style.as_ref() == scene.text_style()
            }) {
                continue;
            }
            let inputs = Inputs {
                scale_factor: self.environment.scale_factor(),
                properties: scene.properties().clone(),
                text_style: scene.text_style().cloned(),
            };
            let payload = (binding.builder)(ModuleMeasureContext {
                scale_factor: inputs.scale_factor,
                registration: entry.kind.registration().expect("registered measurement"),
                properties: &inputs.properties,
                text_style: inputs.text_style.as_ref(),
            });
            let spec = payload.map(|payload| {
                let mut hash = DefaultHasher::new();
                payload.version.hash(&mut hash);
                payload.data.hash(&mut hash);
                MeasurementSpec {
                    content_hash: hash.finish(),
                    style_hash: 0,
                    payload: MeasurementPayload::Custom(payload),
                    pending_policy: PendingMeasurePolicy::Block,
                }
            });
            self.surface.set_measurement(node, spec)?;
            binding.previous = Some(inputs);
        }
        Ok(())
    }
}
