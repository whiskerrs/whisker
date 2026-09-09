use std::collections::BTreeMap;

use whisker_driver_sys::WhiskerValueRaw;
use whisker_engine::whisker_protocol::{
    InlinePlacement, LayoutRect, NodeId, ParagraphLine, ParagraphMetrics, TextFragment, TextRange,
    WhiskerValue,
};

use super::measurement::MobileMeasureError;

type Result<T> = std::result::Result<T, MobileMeasureError>;

#[derive(Default)]
pub(super) struct ParagraphResponse {
    pub placements: Vec<InlinePlacement>,
    pub metrics: Option<ParagraphMetrics>,
}

pub(super) fn decode(raw: *const WhiskerValueRaw) -> Result<ParagraphResponse> {
    if raw.is_null() {
        return Ok(ParagraphResponse::default());
    }
    // SAFETY: the Host retains the response tree until the batch invokes its release callback.
    let value = unsafe { crate::value_codec::decode_value(raw) };
    let fields = fields(&value)?;
    let placements = items(required(fields, "placements")?)?
        .iter()
        .map(|entry| {
            let fields = self::fields(entry)?;
            let node = match required(fields, "node")? {
                WhiskerValue::Int(value) => NodeId::new(*value as u64),
                _ => None,
            }
            .ok_or(MobileMeasureError("invalid inline node"))?;
            let origin = match required(fields, "origin")? {
                WhiskerValue::Null => None,
                WhiskerValue::Array(values) if values.len() == 2 => {
                    Some([number(&values[0])?, number(&values[1])?])
                }
                _ => return Err(MobileMeasureError("invalid inline origin")),
            };
            Ok(InlinePlacement { node, origin })
        })
        .collect::<Result<Vec<_>>>()?;
    let lines = items(required(fields, "lines")?)?
        .iter()
        .map(|entry| {
            let fields = self::fields(entry)?;
            Ok(ParagraphLine {
                range: range(fields)?,
                bounds: rect(required(fields, "bounds")?)?,
                ellipsis_count: integer(required(fields, "ellipsis")?)?,
                baseline: number(required(fields, "baseline")?)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let fragments = items(required(fields, "fragments")?)?
        .iter()
        .map(|entry| {
            let fields = self::fields(entry)?;
            Ok(TextFragment {
                range: range(fields)?,
                bounds: rect(required(fields, "bounds")?)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(ParagraphResponse {
        placements,
        metrics: Some(ParagraphMetrics { lines, fragments }),
    })
}

fn fields(value: &WhiskerValue) -> Result<&BTreeMap<String, WhiskerValue>> {
    match value {
        WhiskerValue::Map(fields) => Ok(fields),
        _ => Err(MobileMeasureError("invalid paragraph object")),
    }
}
fn items(value: &WhiskerValue) -> Result<&[WhiskerValue]> {
    match value {
        WhiskerValue::Array(items) => Ok(items),
        _ => Err(MobileMeasureError("invalid paragraph array")),
    }
}
fn required<'a>(
    fields: &'a BTreeMap<String, WhiskerValue>,
    name: &str,
) -> Result<&'a WhiskerValue> {
    fields
        .get(name)
        .ok_or(MobileMeasureError("missing paragraph geometry field"))
}
fn number(value: &WhiskerValue) -> Result<f32> {
    match value {
        WhiskerValue::Float(value) => Some(*value as f32),
        WhiskerValue::Int(value) => Some(*value as f32),
        _ => None,
    }
    .filter(|value| value.is_finite())
    .ok_or(MobileMeasureError("invalid paragraph coordinate"))
}
fn integer(value: &WhiskerValue) -> Result<u32> {
    match value {
        WhiskerValue::Int(value) => u32::try_from(*value).ok(),
        _ => None,
    }
    .ok_or(MobileMeasureError("invalid paragraph offset"))
}
fn range(fields: &BTreeMap<String, WhiskerValue>) -> Result<TextRange> {
    Ok(TextRange {
        start: integer(required(fields, "start")?)?,
        end: integer(required(fields, "end")?)?,
    })
}
fn rect(value: &WhiskerValue) -> Result<LayoutRect> {
    let values = items(value)?;
    if values.len() != 4 {
        return Err(MobileMeasureError("invalid paragraph rectangle"));
    }
    Ok(LayoutRect {
        x: number(&values[0])?,
        y: number(&values[1])?,
        width: number(&values[2])?,
        height: number(&values[3])?,
    })
}
