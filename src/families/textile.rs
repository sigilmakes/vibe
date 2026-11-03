//! Textile scenes: woven cloth, knitted stitches, embroidery, tartan, lace,
//! patchwork, tapestry, braids, warp tension, and macrame. Patterns are
//! evaluated in cell coordinates, so a thread stays one cell wide at any
//! terminal size. Audio envelopes alter gauge, tension, and thread colour.

use crate::{palette::Theme, visuals::Reactivity};
use ratatui::{buffer::Buffer, layout::Rect};
use std::f32::consts::TAU;

pub const NAMES: &[&str] = &[
    "Warp and Weft",
    "Stockinette",
    "Cross Stitch",
    "Tartan",
    "Lace Mesh",
    "Quilt Blocks",
    "Woven Tapestry",
    "Threefold Braid",
    "Warp Tension",
    "Macramé Knots",
];

const ENV: usize = 7;

#[derive(Default)]
pub struct Renderer {
    last_time: Option<f32>,
    /// Radians, wrapped to a whole number of turns.
    phase: f32,
    /// Abstract motion distance, wrapped to a period shared by every pattern.
    scroll: f32,
    /// Bass, mid, treble, kick, onset, centroid, flatness.
    env: [f32; ENV],
}

fn unit(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn hash(i: i32) -> f32 {
    let mut n = (i as u32)
        .wrapping_mul(747_796_405)
        .wrapping_add(2_891_336_453);
    n = ((n >> ((n >> 28) + 4)) ^ n).wrapping_mul(277_803_737);
    ((n >> 22) ^ n) as f32 / u32::MAX as f32
}

fn line_glyph(tangent: [f32; 2]) -> char {
    let slope = tangent[1] / (tangent[0] + 0.00001);
    if slope.abs() < 0.45 {
        '─'
    } else if slope.abs() > 2.2 {
        '│'
    } else if slope > 0.0 {
        '╲'
    } else {
        '╱'
    }
}

fn seg_dist(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let len2 = d[0] * d[0] + d[1] * d[1];
    let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len2.max(1e-6)).clamp(0.0, 1.0);
    (p[0] - a[0] - d[0] * t).hypot(p[1] - a[1] - d[1] * t)
}

/// Pitch-class centre of mass, so harmony changes the dye lot rather than only
/// flashing on attacks.
fn chroma_hue(audio: &Reactivity) -> f32 {
    let mut weight = 0.0;
    let mut sum = 0.0;
    for (i, c) in audio.chroma.iter().enumerate() {
        let value = unit(*c);
        weight += value;
        sum += value * i as f32;
    }
    if weight > 1e-3 {
        (sum / weight) / 12.0
    } else {
        unit(audio.centroid) * 0.5
    }
}

impl Renderer {
    fn advance(&mut self, time: f32, audio: &Reactivity) {
        let dt = if time.is_finite() {
            let dt = self
                .last_time
                .map_or(1.0 / 60.0, |last| (time - last).clamp(0.0, 0.05));
            self.last_time = Some(time);
            dt
        } else {
            0.0
        };
        let targets = [
            unit(audio.bass),
            unit(audio.mid),
            unit(audio.treble),
            unit(audio.kick),
            unit(audio.onset),
            unit(audio.centroid),
            unit(audio.flatness),
        ];
        let follow = 1.0 - (-dt * 9.0).exp();
        for (value, target) in self.env.iter_mut().zip(targets) {
            *value += (target - *value) * follow;
        }
        self.phase = (self.phase + dt * (0.4 + unit(audio.energy) * 1.2 + self.env[1] * 0.5))
            .rem_euclid(TAU * 16.0);
        self.scroll = (self.scroll + dt * (0.45 + unit(audio.energy) * 1.3 + self.env[3] * 0.4))
            .rem_euclid(20.0);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        time: f32,
        style: usize,
        palette: usize,
        bands: &[f32; 32],
        audio: &Reactivity,
        theme: &Theme,
    ) {
        let mode = style % NAMES.len();
        self.advance(time, audio);
        let visible = area.intersection(buf.area);
        if visible.is_empty() {
            return;
        }
        let t = self.phase;
        let scroll = self.scroll;
        let [bass, mid, treble, kick, onset, centroid, flatness] = self.env;
        let energy = unit(audio.energy);
        let beat = unit(audio.beat);
        let beat_phase = if audio.beat_phase.is_finite() {
            audio.beat_phase.rem_euclid(1.0)
        } else {
            0.0
        };
        let transient = audio
            .transients
            .iter()
            .copied()
            .map(unit)
            .fold(0.0, f32::max);
        let chroma = chroma_hue(audio);
        let cols = visible.width.max(1) as f32;
        let rows = visible.height.max(1) as f32;
        let aspect = cols / rows;
        let background = theme.sample(palette, 0.0, 0.0);

        for row in visible.y..visible.bottom() {
            let gy = (row - visible.y) as f32;
            let v = (gy + 0.5) / rows;
            for col in visible.x..visible.right() {
                let gx = (col - visible.x) as f32;
                let u = (gx + 0.5) / cols;
                let band =
                    unit(bands[((col - visible.x) as usize * 31 / visible.width as usize) % 32]);
                let (brightness, hue, glyph) = match mode {
                    0 => {
                        // Plain weave: a warp and a weft thread alternate which one
                        // covers each crossing.
                        let warp_n = 13.0 + (treble * 7.0).round();
                        let weft_n = 6.0 + (mid * 4.0).round();
                        let wu = u * warp_n;
                        let wv = v * weft_n;
                        let iw = wu.floor();
                        let ih = wv.floor();
                        let fw = wu - iw;
                        let fh = wv - ih;
                        let wobble = 0.03 * (v * 18.0 + t + iw * 0.6).sin();
                        let warp_d = ((fw - 0.5 + wobble).abs() * 2.0).min(1.0);
                        let weft_d = ((fh - 0.5).abs() * 2.0).min(1.0);
                        let width = 0.6 + 0.06 * bass;
                        let warp_s = (1.0 - (warp_d / width).powi(2)).max(0.0).sqrt();
                        let weft_s = (1.0 - (weft_d / width).powi(2)).max(0.0).sqrt();
                        let over = ((iw as i32 + ih as i32) & 1) == 0;
                        let cross = warp_s > 0.3 && weft_s > 0.3;
                        let (s, hue, glyph) = if cross {
                            if over {
                                (warp_s, chroma * 0.5 + iw * 0.055 + 0.1, '┼')
                            } else {
                                (weft_s, chroma * 0.5 + iw * 0.055 + 0.38, '┼')
                            }
                        } else if warp_s > weft_s {
                            (warp_s, chroma * 0.5 + iw * 0.055 + 0.1, '│')
                        } else {
                            (weft_s, chroma * 0.5 + ih * 0.05 + 0.42, '─')
                        };
                        let shade = 0.55 + 0.45 * s;
                        let bright = s * shade * (0.55 + 0.25 * energy + 0.2 * band);
                        (bright, hue.rem_euclid(1.0), glyph)
                    }
                    1 => {
                        // Stockinette: staggered rows of V stitches.
                        let sw = 4.2 - bass * 0.8;
                        let sh = 2.6 + mid * 0.7;
                        let row_f = gy / sh;
                        let stitch_row = row_f.floor();
                        let offset = if (stitch_row as i32) & 1 == 0 {
                            0.0
                        } else {
                            sw * 0.5
                        };
                        let sx = (gx + offset) / sw;
                        let ix = sx.floor();
                        let fx = (sx - ix) * sw;
                        let fy = (row_f - stitch_row) * sh;
                        let bottom = [sw * 0.5, sh * 0.82];
                        let thick = 0.62 + 0.15 * bass;
                        let d = seg_dist([fx, fy], [0.5, 0.35], bottom).min(seg_dist(
                            [fx, fy],
                            [sw - 0.5, 0.35],
                            bottom,
                        ));
                        let s = (1.0 - d / thick).clamp(0.0, 1.0);
                        let hue = chroma * 0.45 + stitch_row * 0.03 + 0.12;
                        let glyph = if s > 0.0 {
                            if fx < sw * 0.5 {
                                '╲'
                            } else {
                                '╱'
                            }
                        } else {
                            ' '
                        };
                        let bright = s * (0.55 + 0.25 * energy + 0.2 * band);
                        (bright, hue.rem_euclid(1.0), glyph)
                    }
                    2 => {
                        // Counted cross stitch with a mirrored sampler motif.
                        let bw = 4.0;
                        let bh = 2.0;
                        let bx = (gx / bw).floor();
                        let by = (gy / bh).floor();
                        let fx = gx - bx * bw;
                        let fy = gy - by * bh;
                        let block_cols = (cols / bw).floor().max(1.0);
                        let block_rows = (rows / bh).floor().max(1.0);
                        let border = bx == 0.0
                            || by == 0.0
                            || bx >= block_cols - 1.0
                            || by >= block_rows - 1.0;
                        let mx = (bx - (block_cols - 1.0) * 0.5).abs().round();
                        let my = (by - (block_rows - 1.0) * 0.5).abs().round();
                        let motif = hash(mx as i32 * 131 + my as i32 * 977 + 5);
                        let active = border
                            || motif < 0.22 + onset * 0.45 + energy * 0.15 + transient * 0.15;
                        let thick = 0.55 + 0.12 * treble;
                        let d1 = seg_dist([fx, fy], [0.6, 0.25], [bw - 0.6, bh - 0.25]);
                        let d2 = seg_dist([fx, fy], [bw - 0.6, 0.25], [0.6, bh - 0.25]);
                        let d = d1.min(d2);
                        let stitch = if active {
                            (1.0 - d / thick).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let aida = if fx < 0.7 && fy < 0.7 { 0.1 } else { 0.0 };
                        let hue = chroma * 0.5 + bx * 0.04 + by * 0.02 + 0.1;
                        let glyph = if stitch > 0.0 {
                            if (d1 - d2).abs() < 0.3 {
                                '╳'
                            } else if d1 < d2 {
                                '╲'
                            } else {
                                '╱'
                            }
                        } else {
                            '·'
                        };
                        let bright = (stitch * (0.6 + 0.25 * energy) + aida) * (0.65 + 0.2 * band);
                        (bright, hue.rem_euclid(1.0), glyph)
                    }
                    3 => {
                        // Tartan sett: hard-edged bands on both axes with a
                        // diagonal twill.
                        let bands_x = 5.0 + (bass * 4.0).round();
                        let bands_y = 4.0 + (mid * 3.0).round();
                        let fx = u * bands_x + beat_phase * 0.08;
                        let fy = v * bands_y;
                        let ix = fx.floor();
                        let iy = fy.floor();
                        let px = fx - ix;
                        let py = fy - iy;
                        let sett = |x: f32| -> f32 {
                            if x < 0.42 {
                                1.0
                            } else if x < 0.62 {
                                0.62
                            } else if x < 0.88 {
                                0.3
                            } else {
                                0.7
                            }
                        };
                        let a = sett(px);
                        let b = sett(py);
                        let twill = ((gx as i32 + gy as i32 + (scroll * 3.0) as i32) % 2) == 0;
                        let bright = (0.16 + 0.42 * a + 0.36 * b - 0.12 * a * b
                            + if twill { 0.06 } else { 0.0 })
                            * (0.65 + 0.25 * energy + 0.15 * band);
                        let hue = chroma * 0.35 + centroid * 0.1 + ix * 0.09 + iy * 0.03 + 0.1;
                        let glyph = if bright > 0.62 {
                            '█'
                        } else if bright > 0.42 {
                            '▓'
                        } else if bright > 0.24 {
                            '▒'
                        } else {
                            '░'
                        };
                        (bright, hue.rem_euclid(1.0), glyph)
                    }
                    4 => {
                        // Fine diagonal net with floral crossings.
                        let x = u * aspect;
                        let y = v;
                        let k = 3.2 + bass * 2.0;
                        let slope = 0.62;
                        let lw = 0.22 + 0.05 * treble + 0.05 * flatness;
                        let a = (x * k + y * k * slope + scroll * 0.1).rem_euclid(1.0) - 0.5;
                        let b = (x * k - y * k * slope).rem_euclid(1.0) - 0.5;
                        let la = (1.0 - a.abs() / lw).clamp(0.0, 1.0);
                        let lb = (1.0 - b.abs() / lw).clamp(0.0, 1.0);
                        let net = la.max(lb);
                        let knot = (la * lb * 4.0).min(1.0);
                        let hue = chroma * 0.55 + (a + b) * 0.15 + 0.12;
                        let glyph = if knot > 0.55 {
                            '✿'
                        } else if la > lb {
                            '╲'
                        } else {
                            '╱'
                        };
                        let bright =
                            net * (0.35 + 0.3 * treble + 0.2 * energy) + knot * (0.3 + 0.3 * onset);
                        (bright, hue.rem_euclid(1.0), glyph)
                    }
                    5 => {
                        // Patchwork blocks with stitched seams and a quilted
                        // diagonal inside each patch.
                        let block_cols = 4.0 + (bass * 2.0).round();
                        let block_rows = 3.0 + mid.round();
                        let bx = (u * block_cols).floor();
                        let by = (v * block_rows).floor();
                        let fu = u * block_cols - bx;
                        let fv = v * block_rows - by;
                        let h = hash(bx as i32 * 131 + by as i32 * 977 + 11);
                        let kind = (h * 4.0) as usize;
                        let patch = match kind {
                            0 => {
                                if fu > fv {
                                    0.72
                                } else {
                                    0.3
                                }
                            }
                            1 => {
                                if (fu * 2.0).floor() as i32 % 2 == 0 {
                                    0.7
                                } else {
                                    0.28
                                }
                            }
                            2 => {
                                if ((fu * 2.0).floor() + (fv * 2.0).floor()) as i32 % 2 == 0 {
                                    0.62
                                } else {
                                    0.26
                                }
                            }
                            _ => {
                                let m = (fu - 0.5).abs().max((fv - 0.5).abs());
                                if (m * 3.0).floor() as i32 % 2 == 0 {
                                    0.66
                                } else {
                                    0.3
                                }
                            }
                        };
                        let block_w = cols / block_cols;
                        let block_h = rows / block_rows;
                        let edge = ((fu.min(1.0 - fu)) * block_w).min((fv.min(1.0 - fv)) * block_h);
                        let dashed = ((gx as i32 + gy as i32) % 4) < 3;
                        let seam = edge < 1.4 && dashed;
                        let quilt = (fu - fv + 0.08 * (fu * 18.0 + t).sin()).abs() < 0.035;
                        let hue = chroma * 0.4 + h * 0.6 + 0.1;
                        let bright = if seam {
                            0.6 + 0.3 * onset
                        } else if quilt {
                            0.35 + 0.2 * treble
                        } else {
                            patch * (0.55 + 0.25 * energy + 0.2 * band)
                        };
                        let glyph = if seam {
                            '┄'
                        } else if quilt {
                            '·'
                        } else if patch > 0.6 {
                            '█'
                        } else if patch > 0.45 {
                            '▓'
                        } else if patch > 0.22 {
                            '▒'
                        } else {
                            '░'
                        };
                        let hue = if seam { hue + 0.3 } else { hue };
                        (bright, hue.rem_euclid(1.0), glyph)
                    }
                    6 => {
                        // Kilim-like tapestry: horizontal weft bands, zigzag
                        // teeth, and a stepped central medallion.
                        let band_h = 2.5 + (mid * 2.0).round();
                        let band_f = gy / band_h;
                        let band = band_f.floor();
                        let fh = band_f - band;
                        let slub = if ((gx as i32 + (band as i32) * 3) & 1) == 0 {
                            1.0
                        } else {
                            0.85
                        };
                        let band_hue = chroma * 0.5 + band * 0.055 + 0.12;
                        let band_bright =
                            (0.2 + 0.12 * (band * 1.3 + scroll * 0.2).sin().abs()) * slub;
                        let x = (u - 0.5) * aspect;
                        let y = v - 0.5;
                        let m = x.abs() * 0.75 + y.abs() * 1.3;
                        let step = 0.07 + 0.015 * treble;
                        let ring = (m / step).floor() as i32 % 2 == 0;
                        let radius = 0.42 + 0.12 * bass + 0.04 * kick;
                        let inside = m < radius;
                        let zig = ((gx / 4.0 + band * 0.5).fract() - 0.5).abs();
                        let teeth = !inside && fh > 0.62 + zig * 0.5;
                        let hue = if inside {
                            chroma * 0.4 + 0.55
                        } else {
                            band_hue
                        };
                        let bright = if inside {
                            if ring {
                                0.72
                            } else {
                                0.32
                            }
                        } else if teeth {
                            0.55
                        } else {
                            band_bright
                        } * (0.7 + 0.2 * energy + 0.15 * band);
                        let glyph = if inside {
                            if ring {
                                '█'
                            } else {
                                '▓'
                            }
                        } else if teeth {
                            '◆'
                        } else {
                            '▒'
                        };
                        (bright, hue.rem_euclid(1.0), glyph)
                    }
                    7 => {
                        // Three strands crossing in repeating braid lanes.
                        let braids = 2.0 + bass.round();
                        let lv = (v * braids).rem_euclid(1.0);
                        let phase = u * TAU * 1.15 + scroll * 0.8 + beat * 0.15;
                        let width = 0.1 + 0.03 * bass;
                        let mut best = 0.0;
                        let mut hue = 0.0;
                        let mut glyph = ' ';
                        for strand in 0..3 {
                            let angle = phase + strand as f32 * TAU / 3.0;
                            let center = 0.5 + 0.36 * angle.sin();
                            let d = (lv - center).abs();
                            let s = (1.0 - d / width).clamp(0.0, 1.0);
                            if s > best {
                                best = s;
                                hue = chroma * 0.5 + strand as f32 / 3.0 + 0.1;
                                let slope = 0.36 * angle.cos() * TAU * 1.15 / braids;
                                glyph = line_glyph([1.0, slope]);
                            }
                        }
                        let bright = best * (0.55 + 0.3 * energy + 0.15 * band);
                        (bright, hue.rem_euclid(1.0), glyph)
                    }
                    8 => {
                        // Warp under tension: vertical threads bent toward a
                        // vibrating pluck point.
                        let spacing = 4.0 + treble * 1.5;
                        let tx = gx / spacing;
                        let idx = tx.round();
                        let base = idx * spacing;
                        let pluck_y = 0.42 + 0.12 * (beat_phase * TAU).sin() + 0.03 * beat;
                        let dy = (v - pluck_y) / (0.16 + 0.05 * bass);
                        let shape = (-dy * dy).exp();
                        let amp = (0.15 + 0.35 * kick + 0.2 * onset) * shape;
                        let wob =
                            amp * (t * 2.0 + idx * 0.55).sin() + 0.04 * (v * 9.0 + idx * 0.4).sin();
                        let center = base + wob * spacing;
                        let d = (gx - center).abs();
                        let width = 0.55 + 0.2 * bass;
                        let thread = (1.0 - d / width).clamp(0.0, 1.0);
                        let tension =
                            0.7 + 0.3 * (v * (28.0 + mid * 16.0) - scroll * 3.0).sin().abs();
                        let px = (u - 0.5) * aspect;
                        let py = v - pluck_y;
                        let glow = (-(px * px + py * py) / 0.004).exp()
                            * (0.3 + 0.5 * kick + 0.3 * transient);
                        let hue = chroma * 0.45 + idx * 0.03 + 0.1;
                        let bright = thread * (0.45 + 0.3 * energy + 0.2 * band) * tension + glow;
                        let glyph = if glow > 0.35 {
                            '✦'
                        } else if (gx - center).abs() < 0.3 {
                            '│'
                        } else if wob > 0.0 {
                            '╱'
                        } else {
                            '╲'
                        };
                        (bright, hue.rem_euclid(1.0), glyph)
                    }
                    _ => {
                        // Macrame: thick diamond cords tied at square knots,
                        // with a swaying fringe below.
                        let x = u * aspect;
                        let y = v;
                        let cell = 0.42 + 0.05 * bass;
                        let a = (x / cell + y / cell).rem_euclid(1.0) - 0.5;
                        let b = (x / cell - y / cell).rem_euclid(1.0) - 0.5;
                        let lw = 0.12 + 0.04 * mid;
                        let la = (1.0 - a.abs() / lw).clamp(0.0, 1.0);
                        let lb = (1.0 - b.abs() / lw).clamp(0.0, 1.0);
                        let knot = (la * lb * 4.0).min(1.0);
                        let cord = la.max(lb);
                        let fringe = if y > 0.82 {
                            let spacing = 0.14 + treble * 0.03;
                            let sway = 0.15 * (t + y * 6.0).sin();
                            let fx = (x / spacing + sway).rem_euclid(1.0);
                            let f = (1.0 - (fx - 0.5).abs() / 0.3).clamp(0.0, 1.0);
                            f * ((y - 0.82) / 0.18).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let hue = chroma * 0.5 + (a + b) * 0.1 + 0.2;
                        let bright = cord * (0.4 + 0.3 * energy + 0.2 * band)
                            + knot * (0.35 + 0.35 * onset + 0.2 * transient)
                            + fringe * (0.3 + 0.25 * treble);
                        let glyph = if knot > 0.35 {
                            '╳'
                        } else if fringe > 0.2 {
                            '│'
                        } else if la > lb {
                            '╲'
                        } else {
                            '╱'
                        };
                        (bright, hue.rem_euclid(1.0), glyph)
                    }
                };

                let cell = &mut buf[(col, row)];
                cell.reset();
                cell.set_bg(background);
                if brightness > 0.03 {
                    cell.set_char(glyph);
                    cell.set_fg(theme.sample(palette, hue, unit(brightness)));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const AREA: Rect = Rect::new(2, 3, 90, 30);

    fn render(style: usize, audio: &Reactivity, frames: usize) -> Buffer {
        let mut renderer = Renderer::default();
        let mut buf = Buffer::empty(AREA);
        for frame in 0..frames {
            renderer.draw(
                &mut buf,
                AREA,
                frame as f32 / 60.0,
                style,
                0,
                &[0.4; 32],
                audio,
                &Theme::default(),
            );
        }
        buf
    }

    fn symbols(buf: &Buffer) -> String {
        buf.content.iter().map(|cell| cell.symbol()).collect()
    }

    fn loud() -> Reactivity {
        Reactivity {
            energy: 1.0,
            bass: 1.0,
            mid: 1.0,
            treble: 1.0,
            onset: 1.0,
            kick: 1.0,
            beat: 1.0,
            beat_phase: 0.5,
            waveform: std::array::from_fn(|i| (i as f32 * 0.7).sin()),
            transients: [1.0; 4],
            novelty: 1.0,
            chroma: std::array::from_fn(|i| (i % 3) as f32 / 2.0),
            centroid: 0.8,
            flatness: 0.7,
            ..Reactivity::default()
        }
    }

    #[test]
    fn names_are_ten_and_unique() {
        assert_eq!(NAMES.len(), 10);
        let unique: HashSet<_> = NAMES.iter().collect();
        assert_eq!(unique.len(), NAMES.len());
    }

    #[test]
    fn every_style_fills_the_area_and_stays_distinct() {
        let mut images = HashSet::new();
        for (style, name) in NAMES.iter().enumerate() {
            let image = symbols(&render(style, &Reactivity::default(), 36));
            assert!(
                image.chars().any(|c| c != ' '),
                "style {style} ({name}) drew nothing"
            );
            assert!(
                images.insert(image),
                "style {style} ({name}) duplicated another scene"
            );
        }
    }

    #[test]
    fn live_audio_changes_each_scene() {
        let quiet = Reactivity::default();
        let loud = loud();
        for style in 0..NAMES.len() {
            let calm = symbols(&render(style, &quiet, 36));
            let hit = symbols(&render(style, &loud, 36));
            assert_ne!(calm, hit, "style {style} ignored the audio");
        }
    }

    #[test]
    fn rendering_clips_to_the_visible_area_and_survives_bad_input() {
        let outer = Rect::new(8, 6, 40, 18);
        for style in 0..NAMES.len() {
            for area in [
                Rect::new(10, 8, 22, 12),
                Rect::new(0, 0, 20, 14),
                Rect::new(70, 30, 9, 9),
                Rect::new(9, 9, 0, 0),
                Rect::new(20, 15, 1, 1),
            ] {
                let mut buf = Buffer::empty(outer);
                for cell in &mut buf.content {
                    cell.set_char('!');
                }
                let audio = Reactivity {
                    bass: f32::NAN,
                    mid: f32::INFINITY,
                    ..Reactivity::default()
                };
                Renderer::default().draw(
                    &mut buf,
                    area,
                    f32::NAN,
                    style,
                    0,
                    &[f32::NAN; 32],
                    &audio,
                    &Theme::default(),
                );
                let visible = area.intersection(outer);
                for y in outer.y..outer.bottom() {
                    for x in outer.x..outer.right() {
                        if !visible.contains((x, y).into()) {
                            assert_eq!(buf[(x, y)].symbol(), "!", "style {style}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn renders_are_deterministic_for_the_same_timeline() {
        for style in 0..NAMES.len() {
            let first = symbols(&render(style, &loud(), 24));
            let second = symbols(&render(style, &loud(), 24));
            assert_eq!(first, second, "style {style} was not deterministic");
        }
    }

    #[test]
    #[ignore = "manual release-mode frame timing"]
    fn release_frame_timing() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 160, 50));
        for (style, name) in NAMES.iter().enumerate() {
            let mut renderer = Renderer::default();
            for frame in 0..120 {
                renderer.draw(
                    &mut buf,
                    Rect::new(0, 0, 160, 50),
                    frame as f32 / 60.0,
                    style,
                    0,
                    &[0.5; 32],
                    &loud(),
                    &Theme::default(),
                );
            }
            let start = std::time::Instant::now();
            for frame in 120..240 {
                renderer.draw(
                    &mut buf,
                    Rect::new(0, 0, 160, 50),
                    frame as f32 / 60.0,
                    style,
                    0,
                    &[0.5; 32],
                    &loud(),
                    &Theme::default(),
                );
            }
            eprintln!(
                "{name}: {:.3} ms/frame",
                start.elapsed().as_secs_f64() * 1000.0 / 120.0
            );
        }
    }
}
