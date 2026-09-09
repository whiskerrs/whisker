# Paragraph layout and rich Text

The outer `Text` owns one paragraph. Nested `Text` elements retain their logical
identity, styles, listeners, and Owner, but do not create separate Host views.
A `View` or `Image` inside the paragraph is an atomic attachment: its subtree is
laid out by Taffy and its position in the line is supplied by the Host's text
layout. A Text inside that View starts an independent paragraph.

## Using Text

Existing `Text(value: ...)` code remains valid. Compose inline styles and atomic
content with the normal children syntax:

```rust
use whisker::prelude::*;
use whisker::css::FontWeight;

render! {
    Text(selectable: true) {
        Text(value: "A paragraph with ")
        Text(value: "emphasis", style: Css::new().font_weight(FontWeight::Bold))
        Text(value: " and an inline badge ")
        View(style: Css::new().padding(px(4))) {
            Text(value: "NEW")
        }
    }
}
```

Values, styles, and children remain reactive. Inline listeners use the existing
capture, bubble, and catch rules; a wrapped link only receives pointer input on
its visible text fragments. Text inside an atomic View starts a new paragraph,
so an icon, badge, or interactive control can use ordinary layout internally.

Place at most one `InlineTruncation` directly inside the outer Text. Its contents
replace the hidden suffix when `max_lines` is exceeded. The token stays mounted
while hidden, preserving local state and subscriptions. It cannot be nested in
another inline Text or used as a standalone root.

Bind `TextHandle::r()` through `Text(element_ref: ...)` when geometry or selection
is needed. Handles on nested inline Text return `TextQueryError::NotParagraph`.

| API | Contract |
| --- | --- |
| `Text::selectable(bool)` | Enables the platform's text-selection interaction |
| `Text::on_selection_change(...)` | Reports the selected UTF-16 range |
| `Text::on_text_layout(...)` | Reports visible lines, truncation counts, and content size after frame acceptance |
| `TextHandle::set_selection(TextRange)` | Queues a validated selection on the displayed paragraph |
| `TextHandle::clear_selection()` | Removes the current selection |
| `TextHandle::selected_text().await` | Reads selected text, substituting explicit attachment labels |
| `TextHandle::bounding_rects(TextRange).await` | Returns visible rectangles relative to the paragraph's content origin |

`TextRange::from_utf8` and `to_utf8` convert Rust string ranges without splitting
Unicode scalar values. Hosts expand nonempty selection to grapheme boundaries.
A range covering hidden text only returns no rectangles. Selection and geometry
queries never include generated truncation text. Handle operations require a
mounted outer Text and accepted layout; a caller can retry `StaleLayout` after
the next presentation.

## Runtime and Host boundary

The Runtime compiles logical children into source text, metric runs, paint runs,
and attachment references. The existing `value` contributes a prefix before the
children, regardless of builder call order. Ordinary attachments contribute one
U+FFFC character to this source. `InlineTruncation` contributes no source text.

The layout adapter keeps the paragraph as a Taffy measurement leaf while resolving
attachment subtrees in the same tree. Auto widths use shrink-to-fit constraints;
percentage widths use the paragraph content width. A percentage height needs an
independent definite paragraph height. A blocked or provisional subtree must not
leave a reusable zero-size cache entry, even within the current layout pass.

The Host shapes the paragraph under those constraints and returns line metrics,
visible source fragments, and an optional origin for every attachment. A missing
origin hides the attachment subtree without disposing its Owner. The Runtime
applies its layout, visibility, and text content in the same accepted frame.
Taffy positions surrounding boxes and uses the Host's first baseline; it does not
shape glyphs or decide line breaks.

| Host | Paragraph backend | Retained preparation |
| --- | --- | --- |
| Android | Spans and StaticLayout | Layout retained through a JNI global reference; TextView supplies native selection |
| iOS | Attributed strings and TextKit | Text storage, layout manager, and text container; UITextView supplies native selection |
| Web | DOM spans and inline blocks | Measured paragraph DOM moved into its presentation container |
| Desktop | Parley and Swash | Shaped lines, positioned glyphs, and inline boxes |

Preparation IDs belong to a surface's measurement cache. The measurement provider
retains the IDs the coordinator can reuse. Mobile responses transfer an opaque
layout lease with an explicit release callback; rejected or partially decoded
batches release leases as well as geometry. Host presentation references can keep
a layout alive after the measurement cache stops using it. These objects contain
no Signal or application callback. Paint updates reuse metric preparation. Rich
preparations are scoped to one paragraph node because their native drawing state
is mutable. Plain text keeps
cross-node measurement deduplication.

## Style placement

`StyleProperty::supports_text_scope` is the machine-readable placement rule.
It distinguishes where a declaration is meaningful from whether a Host implements
all variants of that declaration. Inline Text does not create a CSS box.

| Scope | Properties |
| --- | --- |
| Outer paragraph | Box layout, line height, alignment, indent, white space, word break, maximum lines, overflow, and base text style |
| Inline Text | Fonts and OpenType settings, color, letter spacing, decoration, shadow, background color/radii, and vertical alignment |
| Atomic View/Image | Normal element layout, margins, and vertical alignment in the paragraph |

Box properties and paragraph properties on an inner Text are diagnosed in debug
builds. `word-break` is paragraph-wide. Padding around an unbreakable inline badge
belongs on a View. Partial backgrounds and decorations follow visual fragments,
including fragments separated by wrapping or bidirectional text.

Alignment uses a paragraph font strut. Baseline-aligned and shifted contents
contribute ascent/descent; top/bottom-aligned contents constrain the final line
height. Middle alignment uses the paragraph font's x-height. An attachment with
no usable baseline falls back to its bottom edge.

`WhiteSpace::PreWrap` preserves code indentation and explicit newlines while
allowing wrapping. Font shaping and native selection affordances can differ by
platform; shared fixtures assert geometry and source-mapping invariants rather
than identical system-font pixels.

## Interaction and lifetime

Public selection and query ranges use UTF-16 code units. The protocol converts
those ranges to checked UTF-8 byte ranges and rejects surrogate splits. Hosts
expand nonempty selection endpoints to grapheme boundaries. Attachment copy text
comes only from its explicit accessibility label; unlabeled decorations and the
contents of embedded Inputs are not copied. Custom truncation content is outside
the selectable source.

Pointer hit testing uses visible source fragments and dispatches through the
logical Text tree. It does not join disjoint fragments into a single large hit
rectangle. Events retain the presentation revision at entry, including deferred
input and synthesized activation. A changed paragraph or ancestor geometry
invalidates older pointer input. Paint-only changes preserve interaction geometry.
Native selection owns the long-press gesture of a selectable paragraph.

Assistive technologies receive one paragraph text value and actions for visible
interactive inline ranges. Android/iOS/Desktop expose native accessibility
actions; Web exposes keyboard-focusable inline controls. Atomic children expose
their own semantics. Every inline action returns an opaque span and preparation
ID for Runtime validation before logical tap propagation.

TextHandle commands address an outer paragraph. Async queries carry a correlation
ID and a prepared-content revision, and never block the UI thread. Pending queries
are scoped to the caller's Owner; element removal, a stale layout, malformed Host
responses, or timeout produces a typed error. Late responses cannot call disposed
application code. `on_text_layout` is emitted only after presentation acceptance,
when its reported geometry changes.

## Validation

Compiler and lifecycle integration tests live under
`crates/whisker/tests/surface_render_pipeline/`. Core tests do not depend on example applications.
Host fixtures live under `tests/host-conformance/paragraphs/`; each platform tests
its production paragraph implementation. `cargo xtask mobile-link-test` additionally
checks the Rust/C/Swift/JNI seams.
