use super::*;
use taffy::util::{MaybeResolve, ResolveOrZero};
use taffy::{AvailableSpace, Line, Point, RequestedAxis, SizingMode};

struct InlineBoxLayout {
    backend: NodeId,
    input: LayoutInput,
    output: LayoutOutput,
    margin: taffy::Rect<f32>,
    measured: MeasuredInlineChild,
}

impl<Context: Clone, Measure> LayoutPass<'_, Context, Measure>
where
    Measure: FnMut(
        Size<Option<f32>>,
        Size<AvailableSpace>,
        Option<&Context>,
        &[MeasuredInlineChild],
    ) -> IntrinsicResult,
{
    pub(super) fn compute_measured_leaf(
        &mut self,
        node: NodeId,
        input: LayoutInput,
    ) -> LayoutOutput {
        let context = self.tree.get_node_context(node);
        let style = self.tree.style(node).expect("retained leaf style");
        let inset = content_inset(style, input.parent_size.width);
        let mut measured = None;
        let mut output = compute_leaf_layout(
            input,
            style,
            |_, _| 0.0,
            |known, available| {
                let result = (self.measure)(known, available, context, &[]);
                let size = Size {
                    width: result.size.width,
                    height: result.size.height,
                };
                measured = Some(result);
                size
            },
        );
        if let Some(measured) = measured {
            self.accept_measurement(node, input, &mut output, measured, inset);
        }
        output
    }

    pub(super) fn compute_paragraph(&mut self, node: NodeId, input: LayoutInput) -> LayoutOutput {
        let context = self.tree.get_node_context(node).cloned();
        let style = self
            .tree
            .style(node)
            .expect("retained paragraph style")
            .clone();
        let inset = content_inset(&style, input.parent_size.width);
        let height_is_definite = input.known_dimensions.height.is_some()
            || style
                .size
                .height
                .maybe_resolve(input.parent_size.height, |_, _| 0.0)
                .is_some();
        let children = self.tree.child_ids(node).collect::<Vec<_>>();
        let mut measured = None;
        let mut inline = Vec::new();
        let mut output = compute_leaf_layout(
            input,
            &style,
            |_, _| 0.0,
            |known, available| {
                let before = self.state.blocked;
                for child in children {
                    let mut child_basis = known;
                    if height_is_definite && let AvailableSpace::Definite(height) = available.height
                    {
                        child_basis.height = Some(height.max(0.0));
                    }
                    inline.push(self.measure_inline(child, child_basis, available));
                }
                if self.state.blocked != before {
                    return Size::ZERO;
                }
                let sizes = inline
                    .iter()
                    .map(|child| child.measured)
                    .collect::<Vec<_>>();
                let result = (self.measure)(known, available, context.as_ref(), &sizes);
                let size = Size {
                    width: result.size.width,
                    height: result.size.height,
                };
                measured = Some(result);
                size
            },
        );
        if let Some(measured) = measured {
            if input.run_mode == RunMode::PerformLayout
                && measured.state != MeasurementState::Blocked
            {
                for (order, child) in inline.into_iter().enumerate() {
                    let placement = measured
                        .inline_placements
                        .iter()
                        .find(|placement| placement.node == child.measured.node);
                    let Some(origin) = placement.and_then(|placement| placement.origin) else {
                        self.state.suppressed.insert(child.backend);
                        compute_hidden_layout(self, child.backend);
                        continue;
                    };
                    self.state.suppressed.remove(&child.backend);
                    let style = self
                        .tree
                        .style(child.backend)
                        .expect("retained inline style");
                    let padding = style
                        .padding
                        .resolve_or_zero(child.input.parent_size.width, |_, _| 0.0);
                    let border = style
                        .border
                        .resolve_or_zero(child.input.parent_size.width, |_, _| 0.0);
                    let scrollbar_size = style.overflow.transpose().map(|overflow| {
                        if overflow == taffy::Overflow::Scroll {
                            style.scrollbar_width
                        } else {
                            0.0
                        }
                    });
                    self.set_unrounded_layout(
                        child.backend,
                        &Layout {
                            order: order as u32,
                            location: Point {
                                x: inset.x + origin[0] + child.margin.left,
                                y: inset.y + origin[1] + child.margin.top,
                            },
                            size: child.output.size,
                            content_size: child.output.content_size,
                            scrollbar_size: scrollbar_size.into(),
                            padding,
                            border,
                            margin: child.margin,
                        },
                    );
                }
            }
            self.accept_measurement(node, input, &mut output, measured, inset);
        }
        output
    }

    fn measure_inline(
        &mut self,
        node: NodeId,
        known: Size<Option<f32>>,
        available: Size<AvailableSpace>,
    ) -> InlineBoxLayout {
        let parent_size = Size {
            width: known.width.or(match available.width {
                AvailableSpace::Definite(width) => Some(width),
                _ => None,
            }),
            height: known.height,
        };
        let style = self.tree.style(node).expect("retained inline child style");
        let margin = style.margin.resolve_or_zero(parent_size.width, |_, _| 0.0);
        let auto_width = style.size.width.is_auto();
        let mut input = LayoutInput {
            run_mode: RunMode::ComputeSize,
            sizing_mode: SizingMode::InherentSize,
            axis: RequestedAxis::Both,
            known_dimensions: Size::NONE,
            parent_size,
            available_space: Size {
                width: available.width,
                height: AvailableSpace::MaxContent,
            },
            vertical_margins_are_collapsible: Line::FALSE,
        };
        if auto_width && let Some(width) = parent_size.width {
            input.available_space.width = AvailableSpace::MinContent;
            let minimum = self.compute(node, input, None).size.width;
            input.available_space.width = AvailableSpace::MaxContent;
            let maximum = self.compute(node, input, None).size.width;
            let available = (width - margin.left - margin.right).max(0.0);
            input.known_dimensions.width = Some(maximum.min(minimum.max(available)));
        }
        input.available_space.width = available.width;
        input.run_mode = RunMode::PerformLayout;
        let output = self.compute(node, input, None);
        let measured = MeasuredInlineChild {
            node: self.state.model_nodes[&node],
            size: LayoutSize::new(
                (output.size.width + margin.left + margin.right).max(0.0),
                (output.size.height + margin.top + margin.bottom).max(0.0),
            ),
            baseline: output.first_baselines.y.unwrap_or(output.size.height) + margin.top,
        };
        InlineBoxLayout {
            backend: node,
            input,
            output,
            margin,
            measured,
        }
    }

    fn accept_measurement(
        &mut self,
        node: NodeId,
        input: LayoutInput,
        output: &mut LayoutOutput,
        measured: IntrinsicResult,
        inset: Point<f32>,
    ) {
        output.first_baselines.y = measured.first_baseline.map(|baseline| baseline + inset.y);
        if input.run_mode == RunMode::PerformLayout {
            self.state.nodes.entry(node).or_default().selection = measured.selection;
        }
        match measured.state {
            MeasurementState::Ready => {}
            MeasurementState::Provisional => self.state.provisional += 1,
            MeasurementState::Blocked => self.state.blocked += 1,
        }
    }
}

fn content_inset(style: &Style, parent_width: Option<f32>) -> Point<f32> {
    let padding = style.padding.resolve_or_zero(parent_width, |_, _| 0.0);
    let border = style.border.resolve_or_zero(parent_width, |_, _| 0.0);
    Point {
        x: padding.left + border.left,
        y: padding.top + border.top,
    }
}
