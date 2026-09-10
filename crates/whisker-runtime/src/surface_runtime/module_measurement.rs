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
                hash_value(&payload.data, &mut hash);
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

fn hash_value(value: &WhiskerValue, hash: &mut impl Hasher) {
    std::mem::discriminant(value).hash(hash);
    match value {
        WhiskerValue::Null => {}
        WhiskerValue::Bool(value) => value.hash(hash),
        WhiskerValue::Int(value) => value.hash(hash),
        WhiskerValue::Float(value) => {
            (if *value == 0.0 { 0.0_f64 } else { *value })
                .to_bits()
                .hash(hash);
        }
        WhiskerValue::String(value) | WhiskerValue::Error(value) => value.hash(hash),
        WhiskerValue::Bytes(value) => value.hash(hash),
        WhiskerValue::Array(values) => {
            values.len().hash(hash);
            for value in values {
                hash_value(value, hash);
            }
        }
        WhiskerValue::Map(values) => {
            values.len().hash(hash);
            for (key, value) in values {
                key.hash(hash);
                hash_value(value, hash);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(value: &WhiskerValue) -> u64 {
        let mut state = DefaultHasher::new();
        hash_value(value, &mut state);
        state.finish()
    }

    #[test]
    fn measurement_hash_preserves_value_types_structure_and_float_equality() {
        let variants = [
            WhiskerValue::Null,
            WhiskerValue::Bool(false),
            WhiskerValue::Int(0),
            WhiskerValue::Float(0.0),
            WhiskerValue::String("0".into()),
            WhiskerValue::Bytes(vec![0]),
            WhiskerValue::Array(vec![WhiskerValue::Int(0)]),
            WhiskerValue::map([("n", WhiskerValue::Int(0))]),
            WhiskerValue::Error("0".into()),
        ];
        let hashes: std::collections::HashSet<_> = variants.iter().map(hash).collect();
        assert_eq!(hashes.len(), variants.len());
        assert_eq!(
            hash(&WhiskerValue::Float(0.0)),
            hash(&WhiskerValue::Float(-0.0))
        );
        let nested = |value| {
            WhiskerValue::map([(
                "values",
                WhiskerValue::Array(vec![WhiskerValue::Float(value)]),
            )])
        };
        assert_ne!(hash(&nested(1.0)), hash(&nested(2.0)));
        assert_eq!(hash(&nested(1.0)), hash(&nested(1.0).clone()));
        assert_ne!(
            hash(&WhiskerValue::Array(vec![
                WhiskerValue::Int(1),
                WhiskerValue::Int(2)
            ])),
            hash(&WhiskerValue::Array(vec![
                WhiskerValue::Int(2),
                WhiskerValue::Int(1)
            ]))
        );
    }
}
