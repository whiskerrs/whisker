use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::rc::Rc;

use gtk4::pango;
use gtk4::prelude::*;
use whisker_engine::MeasurementProvider;
use whisker_protocol::{
    AvailableSpace, MeasureFontFamily, MeasureFontStyle, MeasureLineHeight, MeasureTextAlignment,
    MeasureTextDirection, MeasureTextOverflow, MeasureTextWordBreak, MeasureTextWrap, MeasuredSize,
    MeasurementMetrics, MeasurementPayload, MeasurementRequest, MeasurementResponse,
    PreparedContentId, SurfaceId, TextMeasurePayload, UnsupportedMeasurementReason,
};

/// Creates a shaping context on GTK's default font map without a display.
pub(crate) fn new_context() -> pango::Context {
    pangocairo::FontMap::default().create_context()
}

/// Shaped text keyed by the measurement that produced it.
pub(crate) type PreparedLayouts = Rc<RefCell<HashMap<PreparedContentId, pango::Layout>>>;

pub(crate) struct PangoMeasurementProvider {
    context: pango::Context,
    prepared: PreparedLayouts,
    fresh: HashSet<PreparedContentId>,
}

impl PangoMeasurementProvider {
    pub(crate) fn new(context: pango::Context) -> Self {
        Self {
            context,
            prepared: Rc::default(),
            fresh: HashSet::new(),
        }
    }

    pub(crate) fn prepared(&self) -> PreparedLayouts {
        Rc::clone(&self.prepared)
    }

    pub(crate) fn context(&self) -> &pango::Context {
        &self.context
    }

    fn measure_text(
        &self,
        payload: &TextMeasurePayload,
        request: &MeasurementRequest,
    ) -> (pango::Layout, MeasurementMetrics) {
        let available = match request.constraints.available_space[0] {
            AvailableSpace::Definite(value) => Some(value.max(0.0)),
            AvailableSpace::MinContent => Some(0.0),
            AvailableSpace::MaxContent => None,
        };
        let width = request.constraints.known_dimensions[0].or(available);
        let layout = build_layout(&self.context, payload, width);
        let (text_width, text_height) = layout_size(&layout, payload.max_lines);
        let metrics = MeasurementMetrics {
            size: MeasuredSize::new(
                request.constraints.known_dimensions[0].unwrap_or(text_width.ceil()),
                request.constraints.known_dimensions[1].unwrap_or(text_height.ceil()),
            ),
            first_baseline: Some(pango_to_px(layout.baseline())),
            last_baseline: last_baseline(&layout, payload.max_lines),
            ..MeasurementMetrics::from_size(MeasuredSize::default())
        };
        (layout, metrics)
    }
}

impl MeasurementProvider for PangoMeasurementProvider {
    type Error = Infallible;

    fn measure_batch(
        &mut self,
        _surface: SurfaceId,
        requests: &[MeasurementRequest],
        responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        for request in requests {
            let MeasurementPayload::Text(payload) = &request.payload else {
                responses.push(MeasurementResponse::Unsupported {
                    key: request.key,
                    environment_epoch: request.environment_epoch,
                    reason: UnsupportedMeasurementReason::Kind,
                });
                continue;
            };
            let (layout, mut metrics) = self.measure_text(payload, request);
            let id = PreparedContentId::new(request.key.get())
                .expect("measurement keys are always non-zero");
            self.prepared.borrow_mut().insert(id, layout);
            self.fresh.insert(id);
            metrics.prepared_content = Some(id);
            responses.push(MeasurementResponse::Ready {
                key: request.key,
                environment_epoch: request.environment_epoch,
                metrics,
            });
        }
        Ok(())
    }

    fn measures_synchronously(&self) -> bool {
        true
    }

    fn retain_prepared_content(
        &mut self,
        _surface: SurfaceId,
        _revision: u64,
        retained: &mut dyn Iterator<Item = PreparedContentId>,
    ) {
        let mut keep = std::mem::take(&mut self.fresh);
        keep.extend(retained);
        self.prepared.borrow_mut().retain(|id, _| keep.contains(id));
    }
}

pub(crate) fn pango_to_px(value: i32) -> f32 {
    value as f32 / pango::SCALE as f32
}

fn px_to_pango(value: f32) -> i32 {
    (value * pango::SCALE as f32).round() as i32
}

/// Shapes `payload` under an optional inline-size constraint.
pub(crate) fn build_layout(
    context: &pango::Context,
    payload: &TextMeasurePayload,
    width: Option<f32>,
) -> pango::Layout {
    let layout = pango::Layout::new(context);
    layout.set_auto_dir(payload.direction == MeasureTextDirection::Auto);
    if payload.direction != MeasureTextDirection::Auto {
        let context = layout.context();
        context.set_base_dir(if payload.direction == MeasureTextDirection::RightToLeft {
            pango::Direction::Rtl
        } else {
            pango::Direction::Ltr
        });
        layout.context_changed();
    }
    layout.set_font_description(Some(&font_description(payload)));
    let attributes = pango::AttrList::new();
    if let MeasureLineHeight::LogicalPixels(height) = payload.style.line_height {
        attributes.insert(pango::AttrInt::new_line_height_absolute(px_to_pango(
            height,
        )));
    }
    if payload.style.letter_spacing != 0.0 {
        attributes.insert(pango::AttrInt::new_letter_spacing(px_to_pango(
            payload.style.letter_spacing,
        )));
    }
    layout.set_attributes(Some(&attributes));
    layout.set_text(&payload.text);
    layout.set_alignment(match payload.alignment {
        MeasureTextAlignment::Start | MeasureTextAlignment::Left => pango::Alignment::Left,
        MeasureTextAlignment::End | MeasureTextAlignment::Right => pango::Alignment::Right,
        MeasureTextAlignment::Center => pango::Alignment::Center,
    });
    set_layout_width(layout.clone(), payload, width);
    layout
}

/// Applies the inline-size constraint and everything resolved against it.
pub(crate) fn set_layout_width(
    layout: pango::Layout,
    payload: &TextMeasurePayload,
    width: Option<f32>,
) {
    let wraps = payload.wrap != MeasureTextWrap::NoWrap;
    let ellipsizes = payload.overflow == MeasureTextOverflow::Ellipsis;
    match width {
        Some(width) if wraps || ellipsizes => layout.set_width(px_to_pango(width)),
        _ => layout.set_width(-1),
    }
    layout.set_wrap(match payload.word_break {
        MeasureTextWordBreak::BreakAll => pango::WrapMode::Char,
        MeasureTextWordBreak::Normal | MeasureTextWordBreak::KeepAll => pango::WrapMode::Word,
    });
    layout.set_indent(px_to_pango(payload.indent.resolve(width.unwrap_or(0.0))));
    if ellipsizes {
        layout.set_ellipsize(pango::EllipsizeMode::End);
        let lines = if wraps {
            payload.max_lines.unwrap_or(1)
        } else {
            1
        };
        layout.set_height(-(lines.max(1) as i32));
    } else {
        layout.set_ellipsize(pango::EllipsizeMode::None);
        layout.set_height(-1);
    }
}

fn font_description(payload: &TextMeasurePayload) -> pango::FontDescription {
    let mut description = pango::FontDescription::new();
    let families = payload
        .style
        .font_families
        .iter()
        .map(|family| match family {
            MeasureFontFamily::System => "sans-serif",
            MeasureFontFamily::Named(name) => name.as_str(),
        })
        .collect::<Vec<_>>()
        .join(",");
    description.set_family(&families);
    description.set_absolute_size(f64::from(payload.style.font_size) * f64::from(pango::SCALE));
    description.set_weight(match payload.style.font_weight {
        0..=150 => pango::Weight::Thin,
        151..=250 => pango::Weight::Ultralight,
        251..=325 => pango::Weight::Light,
        326..=365 => pango::Weight::Semilight,
        366..=390 => pango::Weight::Book,
        391..=450 => pango::Weight::Normal,
        451..=550 => pango::Weight::Medium,
        551..=650 => pango::Weight::Semibold,
        651..=750 => pango::Weight::Bold,
        751..=850 => pango::Weight::Ultrabold,
        851..=950 => pango::Weight::Heavy,
        _ => pango::Weight::Ultraheavy,
    });
    description.set_style(match payload.style.font_style {
        MeasureFontStyle::Normal => pango::Style::Normal,
        MeasureFontStyle::Italic => pango::Style::Italic,
        MeasureFontStyle::Oblique => pango::Style::Oblique,
    });
    description
}

/// Logical size of the first `max_lines` lines in logical pixels.
pub(crate) fn layout_size(layout: &pango::Layout, max_lines: Option<u32>) -> (f32, f32) {
    let (_, logical) = layout.extents();
    let width = pango_to_px(logical.width());
    let height = match max_lines {
        Some(limit) if (limit as i32) < layout.line_count() => {
            let mut iter = layout.iter();
            let mut bottom = 0;
            for _ in 0..limit {
                let (_, line) = iter.line_extents();
                bottom = line.y() + line.height();
                if !iter.next_line() {
                    break;
                }
            }
            pango_to_px(bottom)
        }
        _ => pango_to_px(logical.height()),
    };
    (width, height)
}

fn last_baseline(layout: &pango::Layout, max_lines: Option<u32>) -> Option<f32> {
    let visible = max_lines.map_or(layout.line_count(), |limit| {
        layout.line_count().min(limit as i32)
    });
    let mut iter = layout.iter();
    for _ in 1..visible {
        if !iter.next_line() {
            break;
        }
    }
    Some(pango_to_px(iter.baseline()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_protocol::{
        ElementTypeId, MeasureConstraints, MeasurementKey, NodeId, TextMeasureStyle,
    };

    fn payload(text: &str) -> TextMeasurePayload {
        TextMeasurePayload {
            runs: Vec::new(),
            attachments: Vec::new(),
            text: text.into(),
            style: TextMeasureStyle::default(),
            locale: None,
            direction: MeasureTextDirection::Auto,
            alignment: MeasureTextAlignment::Start,
            indent: Default::default(),
            wrap: MeasureTextWrap::Wrap,
            word_break: MeasureTextWordBreak::Normal,
            max_lines: None,
            overflow: MeasureTextOverflow::Clip,
        }
    }

    fn request(payload: TextMeasurePayload, width: AvailableSpace) -> MeasurementRequest {
        MeasurementRequest {
            key: MeasurementKey::new(1).unwrap(),
            node: NodeId::new(1).unwrap(),
            element_type: ElementTypeId::new(1).unwrap(),
            environment_epoch: 1,
            constraints: MeasureConstraints {
                known_dimensions: [None, None],
                available_space: [width, AvailableSpace::MaxContent],
            },
            payload: MeasurementPayload::Text(payload),
        }
    }

    fn measure(payload: TextMeasurePayload, width: AvailableSpace) -> MeasurementMetrics {
        let mut provider = PangoMeasurementProvider::new(new_context());
        let mut responses = Vec::new();
        provider
            .measure_batch(
                SurfaceId::new(1).unwrap(),
                &[request(payload, width)],
                &mut responses,
            )
            .unwrap();
        match responses.pop() {
            Some(MeasurementResponse::Ready { metrics, .. }) => metrics,
            other => panic!("unexpected response {other:?}"),
        }
    }

    #[test]
    fn narrower_constraints_wrap_text_onto_more_lines() {
        let text = "the quick brown fox jumps over the lazy dog";
        let wide = measure(payload(text), AvailableSpace::MaxContent);
        let narrow = measure(
            payload(text),
            AvailableSpace::Definite(wide.size.width / 3.0),
        );
        assert!(narrow.size.width <= wide.size.width / 3.0 + 1.0);
        assert!(narrow.size.height > wide.size.height * 2.0);
        assert!(narrow.last_baseline > narrow.first_baseline);
    }

    #[test]
    fn line_limits_bound_the_measured_height() {
        let text = "the quick brown fox jumps over the lazy dog";
        let one_line = measure(payload(text), AvailableSpace::MaxContent)
            .size
            .height;
        let mut limited = payload(text);
        limited.max_lines = Some(2);
        let limited = measure(limited, AvailableSpace::Definite(20.0));
        assert!((limited.size.height - one_line * 2.0).abs() <= 1.0);
    }

    #[test]
    fn absolute_line_height_sets_the_line_box() {
        let mut tall = payload("line");
        tall.style.line_height = MeasureLineHeight::LogicalPixels(40.0);
        assert_eq!(measure(tall, AvailableSpace::MaxContent).size.height, 40.0);
    }

    #[test]
    fn retention_keeps_fresh_and_referenced_layouts_only() {
        let mut provider = PangoMeasurementProvider::new(new_context());
        let mut responses = Vec::new();
        let surface = SurfaceId::new(1).unwrap();
        provider
            .measure_batch(
                surface,
                &[request(payload("a"), AvailableSpace::MaxContent)],
                &mut responses,
            )
            .unwrap();
        let id = PreparedContentId::new(1).unwrap();
        provider.retain_prepared_content(surface, 1, &mut std::iter::empty());
        assert!(provider.prepared.borrow().contains_key(&id));
        provider.retain_prepared_content(surface, 2, &mut std::iter::empty());
        assert!(!provider.prepared.borrow().contains_key(&id));
    }
}
