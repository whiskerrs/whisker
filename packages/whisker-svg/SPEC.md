# whisker-svg display-list binary format — SPEC v1

This document is the **single source of truth** for the byte format
transported through `WhiskerValue::Bytes` between
`crates/whisker-svg-core` (Rust producer) and the per-platform
replayers in `packages/whisker-svg/{ios,android}/`. All three
implementations MUST validate against this document. Changes here
require updating every implementation in lockstep.

## Design goals

1. **Byte-stable** — the same SVG input MUST produce identical bytes
   on every Whisker host (no float canonicalisation tricks, no
   dictionary ordering quirks).
2. **Self-describing version** — a future v2 reader MUST be able to
   detect a v1 stream and degrade gracefully.
3. **Forward-compatible opcode space** — unused opcode ranges are
   reserved for gradients / clipping / masks / text / images so that
   adding them later doesn't require renumbering or a new envelope.
4. **Small** — typical icons (single-colour, 10-20 path commands)
   should fit in 200-500 bytes. No string tables, no padding.
5. **Cheap to decode** — every opcode is fixed-length given its
   header byte, so a replayer can scan with a single `while` loop
   and a `match` table.

## Wire layout

All multi-byte integers and IEEE-754 floats are **little-endian**.
Lengths and offsets are 32-bit unless noted.

```
+--------+--------+--------+--------+--------+--------+
| 'W'    | 'S'    | 'D'    | 'L'    | ver    | flags  |
+--------+--------+--------+--------+--------+--------+
| <opcode 1> <payload 1>                              |
| <opcode 2> <payload 2>                              |
| ...                                                 |
| 0xFF END                                            |
+-----------------------------------------------------+
```

- Bytes 0..3 — magic `"WSDL"` (`0x57 0x53 0x44 0x4C`).
- Byte 4 — `version`. v1 = `0x01`.
- Byte 5 — `flags` reserved. v1 = `0x00`. Readers MUST treat any
  non-zero flag as an unsupported extension and abort replay
  (return all-transparent).
- Bytes 6..N — opcode stream, terminated by `0xFF END`.

A reader that doesn't recognise the magic, or whose `version` is
greater than what it knows, MUST stop replay and report
`unsupported version`. Streams without the trailing `END` are
ill-formed — readers MUST stop at the last successfully decoded op
and report `truncated`.

## Opcode space

```
0x00 – 0x0F   container / canvas state
0x10 – 0x1F   paint state (fill / stroke / opacity)
0x20 – 0x2F   path commands (move / line / curve / close)
0x30 – 0x3F   path execution (fill / stroke / fill+stroke)
0x40 – 0x4F   RESERVED — gradients (linear / radial / stops)
0x50 – 0x5F   RESERVED — clipping (push / pop clip path)
0x60 – 0x6F   RESERVED — masking (push / pop mask)
0x70 – 0x7F   RESERVED — text (font / draw_text)
0x80 – 0x8F   RESERVED — images (raw bitmap blob refs)
0x90 – 0xEF   RESERVED — future
0xF0 – 0xFE   RESERVED — markers / control
0xFF          END of stream
```

A reader encountering an opcode in a RESERVED range MUST stop and
report `unsupported opcode 0x{HH}`. (This is intentionally strict
to avoid silent partial renders when v2 streams reach a v1
replayer; the version byte is the official escape hatch.)

## Opcodes — v1

Coordinates are in **SVG user units** as defined by the source
`<svg>`'s `viewBox`. The replayer is responsible for the user-unit
→ pixel mapping (see "Viewport mapping" below).

### `0x01 SAVE` — push canvas state

No payload. Pushes the current paint state, transform, and clip
stack onto a save stack. Matches `CGContextSaveGState` /
`Canvas.save()`.

### `0x02 RESTORE` — pop canvas state

No payload. Pops the last `SAVE`. Replayer MUST treat an unmatched
`RESTORE` as ill-formed and stop.

### `0x03 CONCAT` — concatenate affine transform

Payload: 6 × `f32` = `[a, b, c, d, tx, ty]`. The matrix is
column-major in the CoreGraphics / Android `Matrix.setValues`
convention:

```
| a  c  tx |
| b  d  ty |
| 0  0  1  |
```

The transform is *prepended* to the current CTM. Apply with
`CGContextConcatCTM` on iOS or `Canvas.concat(Matrix)` on Android.

### `0x04 VIEWPORT` — set the user-unit → pixel mapping

Payload: 4 × `f32` = `[vb_min_x, vb_min_y, vb_width, vb_height]`.
This is the `viewBox` of the source `<svg>`. The replayer
computes the actual pixel transform from this plus the
target view's bounds (see "Viewport mapping"). Emitted exactly
once, at the start of body (before any drawing op).

### `0x10 PAINT_FILL_COLOR` — set fill colour

Payload: 4 × `u8` = `[R, G, B, A]`. Component values are 0..255,
straight-alpha (not premultiplied). Replayers convert to their
native colour type at decode time.

### `0x11 PAINT_STROKE_COLOR` — set stroke colour

Payload: 4 × `u8` = `[R, G, B, A]`. Semantics as `PAINT_FILL_COLOR`.

### `0x12 PAINT_STROKE_WIDTH` — set stroke width

Payload: 1 × `f32`. Width in user units. Replayer applies it
unscaled — the active transform on the canvas decides the
visual width.

### `0x13 PAINT_OPACITY` — set group opacity

Payload: 1 × `f32`, `0.0..=1.0`. Applies multiplicatively to
subsequent fill / stroke alpha until reset by `SAVE` / `RESTORE`.

### `0x14 PAINT_FILL_TINT` — fill = host-supplied tint colour

No payload. Tells the replayer "for subsequent fill operations,
use the host's tint colour (e.g. CSS `color`) instead of any value
set by `PAINT_FILL_COLOR`". Resets to "tint mode" until the next
explicit `PAINT_FILL_COLOR` or a `SAVE`/`RESTORE` flip restores
prior state. This is how SVG `fill="currentColor"` is encoded —
the producer can't know the final tint at compile time, so it
defers to the replayer.

### `0x15 PAINT_STROKE_TINT` — stroke = host-supplied tint

No payload. Same semantics as `PAINT_FILL_TINT` but for stroke
paint.

### `0x20 PATH_BEGIN` — start a new path

No payload. Discards any in-progress path on the path builder.

### `0x21 PATH_MOVE_TO`

Payload: 2 × `f32` = `[x, y]`. Equivalent to SVG `M x,y`.

### `0x22 PATH_LINE_TO`

Payload: 2 × `f32` = `[x, y]`. Equivalent to SVG `L x,y`.

### `0x23 PATH_QUAD_TO`

Payload: 4 × `f32` = `[cx, cy, x, y]`. Equivalent to SVG
`Q cx,cy x,y`.

### `0x24 PATH_CUBIC_TO`

Payload: 6 × `f32` = `[c1x, c1y, c2x, c2y, x, y]`. Equivalent to
SVG `C c1x,c1y c2x,c2y x,y`.

### `0x25 PATH_CLOSE`

No payload. Equivalent to SVG `Z`.

### `0x30 PATH_FILL`

No payload. Fills the in-progress path with the current
`PAINT_FILL_COLOR` (or tint if `PAINT_FILL_TINT` was set). Uses
the **non-zero winding** fill rule. (Even-odd is reserved for a
future `0x31 PATH_FILL_EVEN_ODD`.)

### `0x32 PATH_STROKE`

No payload. Strokes the in-progress path with the current
`PAINT_STROKE_COLOR` (or tint) and `PAINT_STROKE_WIDTH`. Caps =
butt, joins = miter (matches SVG defaults). v2 may extend.

### `0x33 PATH_FILL_AND_STROKE`

No payload. Fills then strokes the same path. Semantically the
same as `PATH_FILL` followed by `PATH_STROKE` but the replayer
MAY (and CoreGraphics WILL) fuse them into a single
`CGContextDrawPath(kCGPathFillStroke)` call.

### `0xFF END`

No payload. Marks the end of the body. Anything after END MUST be
ignored. A stream without END is `truncated`.

## Viewport mapping

The SVG `viewBox` (emitted as `VIEWPORT`) defines the user-unit
coordinate system the producer used. The replayer is responsible
for mapping that into the actual view bounds, mirroring the
`preserveAspectRatio="xMidYMid meet"` SVG default:

```
target_w, target_h = view bounds (pixels)
vb_x, vb_y, vb_w, vb_h = viewport opcode payload
scale = min(target_w / vb_w, target_h / vb_h)
tx = (target_w - vb_w * scale) / 2 - vb_x * scale
ty = (target_h - vb_h * scale) / 2 - vb_y * scale
```

The replayer applies `[scale, 0, 0, scale, tx, ty]` as the
**initial** CTM before processing any body opcode. Subsequent
`CONCAT`s prepend to this. v2 may add an opcode to override
`preserveAspectRatio`.

## Tint propagation

CSS `color` on the host `<Svg>` element is the "tint". For
`PAINT_FILL_TINT` / `PAINT_STROKE_TINT` opcodes, the replayer
substitutes the tint colour (resolved from `style="color: …"` or
the inherited Lynx text colour) as the paint. Producers compile
SVG `fill="currentColor"` / `stroke="currentColor"` to these
opcodes.

The tint is **not** stored in the display list — it's a runtime
property of the host element, queried at replay time. A
`PAINT_FILL_COLOR` after a `PAINT_FILL_TINT` overrides the tint
mode for subsequent fills.

## Limits

- A v1 stream MUST NOT exceed 16 MiB. Beyond that the producer
  MUST split or refuse. (Replayers can assume `usize` indexing
  fits.)
- The save stack depth is implementation-defined. Replayers SHOULD
  support at least 64.
- Path commands per stream: unbounded (limited only by the 16 MiB
  envelope).

## Validation table

Conformance test data lives in `crates/whisker-svg-core/tests/fixtures/`:

| Fixture                  | Purpose                                    |
|--------------------------|--------------------------------------------|
| `rect_solid.svg`         | viewBox + single rect + solid fill         |
| `path_triangle.svg`      | path with M/L/Z, solid fill                |
| `path_quad.svg`          | quadratic Bézier (`Q`)                     |
| `path_cubic.svg`         | cubic Bézier (`C`)                         |
| `stroke_outline.svg`     | path with stroke + stroke-width            |
| `currentcolor.svg`       | `fill="currentColor"` tint propagation     |
| `nested_transform.svg`   | nested `<g transform=…>`                   |
| `opacity_group.svg`      | `<g opacity="…">` save/restore semantics   |

For each fixture, both Rust and platform replayer tests load the
file, run the producer, and verify a canonical opcode trace. The
trace is captured as `<fixture>.trace.txt` next to the SVG so a
human review of changes is straightforward — any byte-level format
change MUST update the trace files in the same commit.

## Future opcodes (reserved, NOT IMPLEMENTED IN v1)

Listed here only to lock in the planned layout so that adding
them doesn't conflict with existing opcodes:

```
0x31 PATH_FILL_EVEN_ODD              — even-odd fill rule
0x40 PAINT_FILL_LINEAR_GRADIENT      — payload: stop count + stops + endpoints
0x41 PAINT_FILL_RADIAL_GRADIENT      — payload: stops + centre + radii
0x50 CLIP_PUSH_PATH                  — clip subsequent ops to current path
0x51 CLIP_POP                        — pop one clip layer
0x60 MASK_PUSH                       — start mask group
0x61 MASK_POP                        — apply accumulated mask
0x70 TEXT_FONT                       — payload: family / size / weight / style
0x71 TEXT_DRAW                       — payload: utf-8 length + bytes
0x80 IMAGE_RAW                       — payload: format + w + h + bytes
```

Reservations are advisory — when v2 lands these can be reassigned
if a better layout is found, with the v2 magic byte signalling
the change.
