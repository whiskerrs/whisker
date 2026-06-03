//! SVG `<path d="…">` attribute parser.
//!
//! Decodes the textual command syntax (`M 10 10 L 20 20 Z`) into a
//! sequence of [`PathCommand`]s — uppercase commands are
//! absolute, lowercase are relative; H/V are reduced to LineTo,
//! S/T are resolved against the previous control point (per SVG
//! spec), and A (arc) is approximated by 1-4 cubic Béziers.
//!
//! Errors are tolerant: a malformed token stops at the last
//! successfully decoded command and returns what we have. Icons in
//! the wild are surprisingly chatty with stray whitespace and the
//! occasional missing comma; refusing the entire path on the
//! first oddness would make adoption painful.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathCommand {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    QuadTo(f32, f32, f32, f32),
    CubicTo(f32, f32, f32, f32, f32, f32),
    Close,
}

/// Parse an SVG path `d` attribute. Returns the absolute,
/// normalised command sequence — every relative command has been
/// resolved against the moving pen, every smooth (S/T) command
/// has had its implicit control point computed, and arcs (A) have
/// been approximated by cubics.
pub fn parse(d: &str) -> Vec<PathCommand> {
    let mut out: Vec<PathCommand> = Vec::new();
    let mut p = Parser::new(d);
    // Pen position. Reset by every M / m. Tracked through all
    // commands so relative offsets resolve correctly.
    let mut cx: f32 = 0.0;
    let mut cy: f32 = 0.0;
    // Subpath start — Close returns here.
    let mut sx: f32 = 0.0;
    let mut sy: f32 = 0.0;
    // Last cubic control point (for S smoothing).
    let mut last_c2: Option<(f32, f32)> = None;
    // Last quadratic control point (for T smoothing).
    let mut last_q: Option<(f32, f32)> = None;
    // Previous explicit command, for repeated implicit commands
    // (e.g. `M 0 0 10 10` after the initial moveto switches to
    // implicit LineTo per SVG spec).
    let mut last_cmd: u8 = 0;

    while let Some(cmd) = p.peek_command() {
        // Set the implicit-followup command and skip the letter.
        p.skip_command();
        let mut upper = cmd.to_ascii_uppercase();
        let abs = cmd.is_ascii_uppercase();
        last_cmd = upper;

        loop {
            match upper {
                b'M' => {
                    let Some(x) = p.coord() else { break };
                    let Some(y) = p.coord() else { break };
                    let (tx, ty) = resolve(x, y, abs, cx, cy);
                    out.push(PathCommand::MoveTo(tx, ty));
                    cx = tx;
                    cy = ty;
                    sx = tx;
                    sy = ty;
                    last_c2 = None;
                    last_q = None;
                    // Per SVG spec, additional coord pairs after M
                    // become L (preserving absolute / relative). Switch
                    // `upper` so the next inner-loop iteration picks
                    // the L branch instead of repeating M.
                    upper = b'L';
                    last_cmd = b'L';
                }
                b'L' => {
                    let Some(x) = p.coord() else { break };
                    let Some(y) = p.coord() else { break };
                    let (tx, ty) = resolve(x, y, abs, cx, cy);
                    out.push(PathCommand::LineTo(tx, ty));
                    cx = tx;
                    cy = ty;
                    last_c2 = None;
                    last_q = None;
                }
                b'H' => {
                    let Some(x) = p.coord() else { break };
                    let tx = if abs { x } else { cx + x };
                    out.push(PathCommand::LineTo(tx, cy));
                    cx = tx;
                    last_c2 = None;
                    last_q = None;
                }
                b'V' => {
                    let Some(y) = p.coord() else { break };
                    let ty = if abs { y } else { cy + y };
                    out.push(PathCommand::LineTo(cx, ty));
                    cy = ty;
                    last_c2 = None;
                    last_q = None;
                }
                b'C' => {
                    let (Some(c1x), Some(c1y), Some(c2x), Some(c2y), Some(x), Some(y)) = (
                        p.coord(),
                        p.coord(),
                        p.coord(),
                        p.coord(),
                        p.coord(),
                        p.coord(),
                    ) else {
                        break;
                    };
                    let (a1x, a1y) = resolve(c1x, c1y, abs, cx, cy);
                    let (a2x, a2y) = resolve(c2x, c2y, abs, cx, cy);
                    let (tx, ty) = resolve(x, y, abs, cx, cy);
                    out.push(PathCommand::CubicTo(a1x, a1y, a2x, a2y, tx, ty));
                    cx = tx;
                    cy = ty;
                    last_c2 = Some((a2x, a2y));
                    last_q = None;
                }
                b'S' => {
                    // Smooth cubic: implicit first control point is the
                    // reflection of the previous cubic's c2 through the
                    // pen position (or the pen itself if the previous
                    // command wasn't a cubic).
                    let (Some(c2x), Some(c2y), Some(x), Some(y)) =
                        (p.coord(), p.coord(), p.coord(), p.coord())
                    else {
                        break;
                    };
                    let (rc1x, rc1y) = match last_c2 {
                        Some((px, py)) => (2.0 * cx - px, 2.0 * cy - py),
                        None => (cx, cy),
                    };
                    let (a2x, a2y) = resolve(c2x, c2y, abs, cx, cy);
                    let (tx, ty) = resolve(x, y, abs, cx, cy);
                    out.push(PathCommand::CubicTo(rc1x, rc1y, a2x, a2y, tx, ty));
                    cx = tx;
                    cy = ty;
                    last_c2 = Some((a2x, a2y));
                    last_q = None;
                }
                b'Q' => {
                    let (Some(cqx), Some(cqy), Some(x), Some(y)) =
                        (p.coord(), p.coord(), p.coord(), p.coord())
                    else {
                        break;
                    };
                    let (a1x, a1y) = resolve(cqx, cqy, abs, cx, cy);
                    let (tx, ty) = resolve(x, y, abs, cx, cy);
                    out.push(PathCommand::QuadTo(a1x, a1y, tx, ty));
                    cx = tx;
                    cy = ty;
                    last_q = Some((a1x, a1y));
                    last_c2 = None;
                }
                b'T' => {
                    // Smooth quad: implicit control point is the
                    // reflection of the previous quad's control through
                    // the pen position (or the pen itself).
                    let (Some(x), Some(y)) = (p.coord(), p.coord()) else {
                        break;
                    };
                    let (rcx, rcy) = match last_q {
                        Some((px, py)) => (2.0 * cx - px, 2.0 * cy - py),
                        None => (cx, cy),
                    };
                    let (tx, ty) = resolve(x, y, abs, cx, cy);
                    out.push(PathCommand::QuadTo(rcx, rcy, tx, ty));
                    cx = tx;
                    cy = ty;
                    last_q = Some((rcx, rcy));
                    last_c2 = None;
                }
                b'A' => {
                    let (
                        Some(rx),
                        Some(ry),
                        Some(x_axis_rot),
                        Some(large),
                        Some(sweep),
                        Some(x),
                        Some(y),
                    ) = (
                        p.coord(),
                        p.coord(),
                        p.coord(),
                        p.flag(),
                        p.flag(),
                        p.coord(),
                        p.coord(),
                    )
                    else {
                        break;
                    };
                    let (ex, ey) = resolve(x, y, abs, cx, cy);
                    arc_to_cubics(
                        cx,
                        cy,
                        rx,
                        ry,
                        x_axis_rot.to_radians(),
                        large != 0.0,
                        sweep != 0.0,
                        ex,
                        ey,
                        &mut out,
                    );
                    cx = ex;
                    cy = ey;
                    last_c2 = None;
                    last_q = None;
                }
                b'Z' => {
                    out.push(PathCommand::Close);
                    cx = sx;
                    cy = sy;
                    last_c2 = None;
                    last_q = None;
                    // Z takes no coords — break out so the outer loop
                    // moves to the next command letter.
                    break;
                }
                _ => break,
            }
            // After consuming one command's worth of coords, peek at
            // the next byte: if it's a digit / sign / dot, SVG repeats
            // the previous command implicitly. Otherwise drop to the
            // outer loop so the next letter resets `cmd`.
            if !p.peek_implicit_continuation() {
                break;
            }
        }
    }
    let _ = last_cmd;
    out
}

#[inline]
fn resolve(x: f32, y: f32, abs: bool, cx: f32, cy: f32) -> (f32, f32) {
    if abs {
        (x, y)
    } else {
        (cx + x, cy + y)
    }
}

/// Convert an SVG arc endpoint to one or more cubic Béziers using
/// the standard "centre parametrisation" decomposition. The arc is
/// split into ≤ 90° sub-arcs and each sub-arc approximated by one
/// cubic; the approximation error against a true ellipse is below
/// 1 part in 10000 for sub-arc angles ≤ 90°, more than enough for
/// icon rendering at any reasonable size.
#[allow(clippy::too_many_arguments)]
fn arc_to_cubics(
    x1: f32,
    y1: f32,
    rx_in: f32,
    ry_in: f32,
    phi: f32,
    large: bool,
    sweep: bool,
    x2: f32,
    y2: f32,
    out: &mut Vec<PathCommand>,
) {
    let mut rx = rx_in.abs();
    let mut ry = ry_in.abs();
    if rx < f32::EPSILON || ry < f32::EPSILON {
        out.push(PathCommand::LineTo(x2, y2));
        return;
    }

    // Step 1: compute (x1', y1') — see SVG impl-notes B.2.4
    let dx = (x1 - x2) / 2.0;
    let dy = (y1 - y2) / 2.0;
    let cos_phi = phi.cos();
    let sin_phi = phi.sin();
    let x1p = cos_phi * dx + sin_phi * dy;
    let y1p = -sin_phi * dx + cos_phi * dy;

    // Step 2: ensure radii large enough
    let lam = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lam > 1.0 {
        let s = lam.sqrt();
        rx *= s;
        ry *= s;
    }

    // Step 3: compute centre (cx', cy') in rotated space
    let sign = if large == sweep { -1.0 } else { 1.0 };
    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let factor = if den > 0.0 {
        (num / den).max(0.0).sqrt()
    } else {
        0.0
    };
    let cxp = sign * factor * (rx * y1p / ry);
    let cyp = sign * factor * -(ry * x1p / rx);

    // Step 4: compute centre in original space
    let cx = cos_phi * cxp - sin_phi * cyp + (x1 + x2) / 2.0;
    let cy = sin_phi * cxp + cos_phi * cyp + (y1 + y2) / 2.0;

    // Step 5: compute start angle + sweep
    let ang = |ux: f32, uy: f32, vx: f32, vy: f32| -> f32 {
        let dot = ux * vx + uy * vy;
        let len = ((ux * ux + uy * uy) * (vx * vx + vy * vy)).sqrt();
        let mut a = (dot / len).clamp(-1.0, 1.0).acos();
        if ux * vy - uy * vx < 0.0 {
            a = -a;
        }
        a
    };
    let theta1 = ang(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut dtheta = ang(
        (x1p - cxp) / rx,
        (y1p - cyp) / ry,
        (-x1p - cxp) / rx,
        (-y1p - cyp) / ry,
    );
    if !sweep && dtheta > 0.0 {
        dtheta -= std::f32::consts::TAU;
    } else if sweep && dtheta < 0.0 {
        dtheta += std::f32::consts::TAU;
    }

    // Split into ≤ 90° pieces and emit cubics.
    let segs = ((dtheta.abs() / (std::f32::consts::FRAC_PI_2)).ceil() as usize).max(1);
    let step = dtheta / segs as f32;
    let k = (4.0 / 3.0) * (step / 4.0).tan();
    for i in 0..segs {
        let a0 = theta1 + step * i as f32;
        let a1 = a0 + step;
        let p0 = (
            cx + rx * cos_phi * a0.cos() - ry * sin_phi * a0.sin(),
            cy + rx * sin_phi * a0.cos() + ry * cos_phi * a0.sin(),
        );
        let p1 = (
            cx + rx * cos_phi * a1.cos() - ry * sin_phi * a1.sin(),
            cy + rx * sin_phi * a1.cos() + ry * cos_phi * a1.sin(),
        );
        let dp0 = (
            -rx * cos_phi * a0.sin() - ry * sin_phi * a0.cos(),
            -rx * sin_phi * a0.sin() + ry * cos_phi * a0.cos(),
        );
        let dp1 = (
            -rx * cos_phi * a1.sin() - ry * sin_phi * a1.cos(),
            -rx * sin_phi * a1.sin() + ry * cos_phi * a1.cos(),
        );
        let c1 = (p0.0 + k * dp0.0, p0.1 + k * dp0.1);
        let c2 = (p1.0 - k * dp1.0, p1.1 - k * dp1.1);
        out.push(PathCommand::CubicTo(c1.0, c1.1, c2.0, c2.1, p1.0, p1.1));
    }
}

// ---- low-level tokenizer ---------------------------------------------------

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            bytes: s.as_bytes(),
            pos: 0,
        }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() {
            let c = self.bytes[self.pos];
            if matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b',') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek_command(&mut self) -> Option<u8> {
        self.skip_ws();
        if self.pos >= self.bytes.len() {
            return None;
        }
        let c = self.bytes[self.pos];
        match c {
            b'M' | b'm' | b'L' | b'l' | b'H' | b'h' | b'V' | b'v' | b'C' | b'c' | b'S' | b's'
            | b'Q' | b'q' | b'T' | b't' | b'A' | b'a' | b'Z' | b'z' => Some(c),
            _ => None,
        }
    }

    fn skip_command(&mut self) {
        self.pos += 1;
    }

    fn peek_implicit_continuation(&mut self) -> bool {
        self.skip_ws();
        if self.pos >= self.bytes.len() {
            return false;
        }
        let c = self.bytes[self.pos];
        matches!(c, b'-' | b'+' | b'.' | b'0'..=b'9')
    }

    /// Read one number. Returns None when no number is available.
    fn coord(&mut self) -> Option<f32> {
        self.skip_ws();
        let start = self.pos;
        if self.pos >= self.bytes.len() {
            return None;
        }
        if matches!(self.bytes[self.pos], b'-' | b'+') {
            self.pos += 1;
        }
        let mut saw_digit = false;
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
            self.pos += 1;
            saw_digit = true;
        }
        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'.' {
            self.pos += 1;
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
                saw_digit = true;
            }
        }
        if self.pos < self.bytes.len()
            && (self.bytes[self.pos] == b'e' || self.bytes[self.pos] == b'E')
        {
            self.pos += 1;
            if self.pos < self.bytes.len() && matches!(self.bytes[self.pos], b'-' | b'+') {
                self.pos += 1;
            }
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        if !saw_digit {
            self.pos = start;
            return None;
        }
        std::str::from_utf8(&self.bytes[start..self.pos])
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
    }

    /// 0/1 flag for arc large/sweep arguments. SVG allows the flag
    /// to be a single digit with no separator after it
    /// (`a 10 10 0 010 10`), so we read exactly one byte.
    fn flag(&mut self) -> Option<f32> {
        self.skip_ws();
        if self.pos >= self.bytes.len() {
            return None;
        }
        let c = self.bytes[self.pos];
        match c {
            b'0' => {
                self.pos += 1;
                Some(0.0)
            }
            b'1' => {
                self.pos += 1;
                Some(1.0)
            }
            _ => None,
        }
    }
}
