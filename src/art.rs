//! The start screen's pixel art: a planet turning, its rim crumbling into
//! sparks, stars twinkling round it. A pure function of size and time; `ui`
//! draws it two pixels per terminal cell with the `▀` half block, which makes
//! the pixels roughly square.

use std::f32::consts::{PI, TAU};

use ratatui::style::Color;

/// Deep water to cloud tops.
const SEA: [u8; 6] = [23, 30, 37, 79, 158, 195];
/// A star from a faint dot to a bright plus.
const STAR: [u8; 4] = [94, 136, 221, 230];
const DUST: [u8; 2] = [235, 237];
const BAYER: [f32; 16] = [
    0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0,
];

/// A rectangle, in pixels, where only the planet may be drawn (the menu).
#[derive(Clone, Copy)]
pub struct KeepOut {
    pub x0: usize,
    pub y0: usize,
    pub x1: usize,
    pub y1: usize,
}

impl KeepOut {
    fn holds(self, x: usize, y: usize) -> bool {
        (self.x0..self.x1).contains(&x) && (self.y0..self.y1).contains(&y)
    }
}

/// 0..1, the same for the same input on every run.
fn hash(x: i32, y: i32) -> f32 {
    let mut n = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    ((n ^ (n >> 16)) & 0xffff) as f32 / 65_535.0
}

fn noise(x: f32, y: f32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (xf, yf) = (x - x.floor(), y - y.floor());
    let (u, v) = (xf * xf * (3.0 - 2.0 * xf), yf * yf * (3.0 - 2.0 * yf));
    let (a, b) = (hash(xi, yi), hash(xi + 1, yi));
    let (c, d) = (hash(xi, yi + 1), hash(xi + 1, yi + 1));
    a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v
}

fn fbm(x: f32, y: f32) -> f32 {
    noise(x, y) * 0.5
        + noise(x * 2.1, y * 2.1) * 0.27
        + noise(x * 4.3, y * 4.3) * 0.15
        + noise(x * 8.7, y * 8.7) * 0.08
}

/// `w` x `h` pixels at animation tick `t` (one per 80 ms), row by row.
/// `None` is empty sky.
pub fn sky(w: usize, h: usize, t: usize, keep_out: KeepOut) -> Vec<Option<Color>> {
    let mut px = vec![None; w * h];
    if w == 0 || h == 0 {
        return px;
    }
    let tf = t as f32;
    let put = |x: i64, y: i64, c: u8, px: &mut [Option<Color>]| {
        if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
            px[y as usize * w + x as usize] = Some(Color::Indexed(c));
        }
    };

    // dust: faint specks that blink now and then
    for i in 0..(w * h / 420) as i32 {
        let (x, y) = ((hash(i, 11) * w as f32) as usize, (hash(i, 12) * h as f32) as usize);
        let on = (tf * 0.048 + hash(i, 13) * TAU).sin() > -0.6;
        if on && !keep_out.holds(x, y) {
            put(x as i64, y as i64, DUST[(hash(i, 14) * 2.0) as usize % 2], &mut px);
        }
    }

    // the planet, rising from the bottom edge
    let r = (w as f32 * 0.27).min(h as f32 * 0.62).max(6.0);
    let (cx, cy) = (w as f32 / 2.0, h as f32 + r * 0.18);
    let spin = tf * 0.02;
    for y in ((cy - r).max(0.0) as usize)..h {
        let ny = (y as f32 - cy) / r;
        for x in ((cx - r).max(0.0) as usize)..((cx + r) as usize + 1).min(w) {
            let nx = (x as f32 - cx) / r;
            let rr = nx * nx + ny * ny;
            if rr > 1.0 {
                continue;
            }
            // the rim crumbles: an ordered dither eats the last few pixels
            let dist = rr.sqrt();
            if dist > 0.9 {
                let edge = (dist - 0.9) / 0.1;
                let jitter = noise(x as f32 * 0.7 + tf * 0.16, y as f32 * 0.7) * 0.3;
                if edge + jitter > BAYER[(y & 3) * 4 + (x & 3)] / 16.0 + 0.55 {
                    continue;
                }
            }
            let nz = (1.0 - rr).sqrt();
            let lon = nx.atan2(nz) + spin;
            let lat = ny.asin();
            let land = fbm(lon * 2.2 + 40.0, lat * 3.2 + 40.0);
            let light = (-0.45 * nx - 0.55 * ny + 0.7 * nz).max(0.0);
            let v = land * 0.95 + light * 0.38 - 0.3;
            let shade = ((v * SEA.len() as f32).max(0.0) as usize).min(SEA.len() - 1);
            put(x as i64, y as i64, SEA[shade], &mut px);
        }
    }

    // stars: plus shapes that swell and fade, never on the planet or the menu
    for i in 0..(w * h / 1500 + 6) as i32 {
        let (x, y) = ((hash(i, 1) * w as f32) as usize, (hash(i, 2) * h as f32 * 0.8) as usize);
        let (dx, dy) = (x as f32 - cx, y as f32 - cy);
        if dx * dx + dy * dy < (r + 6.0) * (r + 6.0) || keep_out.holds(x, y) {
            continue;
        }
        let k = ((tf * 0.08 * (0.6 + hash(i, 3) * 1.4) + hash(i, 4) * TAU).sin() + 1.0) / 2.0;
        let level = ((k * 4.0) as usize).min(3);
        let (x, y) = (x as i64, y as i64);
        put(x, y, STAR[level], &mut px);
        if level >= 1 {
            for (ox, oy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                put(x + ox, y + oy, STAR[level - 1], &mut px);
            }
        }
        if level == 3 {
            for (ox, oy) in [(-2, 0), (2, 0)] {
                put(x + ox, y + oy, STAR[1], &mut px);
            }
        }
    }

    // sparks flaking off the upper rim; each one is born, drifts and dies
    // on its own schedule, so nothing needs remembering between frames
    for i in 0..24i32 {
        let period = 34 + (i * 7 % 15) as usize;
        let shifted = t + i as usize * 13;
        let (age, round) = (shifted % period, (shifted / period) as i32);
        if age >= 30 {
            continue;
        }
        let a = PI + hash(i, round) * PI;
        let (dx, dy) = (a.cos(), a.sin());
        let d = age as f32;
        let x = cx + dx * (r + d * 0.4);
        let y = cy + dy * (r + d * 0.4) - d * 0.13;
        let c = match age {
            0..12 => SEA[5],
            12..22 => SEA[4],
            _ => SEA[2],
        };
        let (x, y) = (x.round() as i64, y.round() as i64);
        if x >= 0 && y >= 0 && !keep_out.holds(x as usize, y as usize) {
            put(x, y, c, &mut px);
        }
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_planet_sits_at_the_bottom_and_the_menu_stays_clear() {
        let none = KeepOut { x0: 0, y0: 0, x1: 0, y1: 0 };
        let px = sky(120, 60, 5, none);
        assert_eq!(px.len(), 120 * 60);
        assert!(px[59 * 120 + 60].is_some(), "bottom centre is planet");
        assert_eq!(px, sky(120, 60, 5, none), "same input, same picture");
        assert_ne!(px, sky(120, 60, 40, none), "it moves");

        let menu = KeepOut { x0: 30, y0: 0, x1: 90, y1: 20 };
        let px = sky(120, 60, 5, menu);
        let lit = (0..20).flat_map(|y| (30..90).map(move |x| (x, y))).filter(|(x, y)| px[y * 120 + x].is_some());
        assert_eq!(lit.count(), 0, "nothing drawn behind the menu");
        assert!(sky(0, 0, 0, none).is_empty());
    }
}
