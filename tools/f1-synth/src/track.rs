//! A procedurally generated closed circuit with a physically plausible
//! speed profile (cornering limit + acceleration/braking passes).

use std::f32::consts::TAU;

pub struct Track {
    pub length: f32,
    /// Sample spacing in metres.
    ds: f32,
    xs: Vec<f32>,
    zs: Vec<f32>,
    /// Speed limit (m/s) at each sample.
    pub speed: Vec<f32>,
    /// Signed curvature (1/m) at each sample.
    curvature: Vec<f32>,
}

const SAMPLES: usize = 2600;

impl Track {
    pub fn generate(target_length_m: f32) -> Self {
        // Raw parametric outline.
        let n_raw = 8000;
        let shape = |t: f32| -> (f32, f32) {
            let r = 1.0 + 0.30 * (2.0 * t).sin() + 0.12 * (3.0 * t + 0.7).sin() + 0.05 * (5.0 * t + 1.3).sin() + 0.035 * (8.0 * t + 2.1).sin();
            (r * t.cos(), r * t.sin() * 0.75)
        };
        let raw: Vec<(f32, f32)> = (0..=n_raw).map(|i| shape(i as f32 / n_raw as f32 * TAU)).collect();
        let raw_len: f32 = raw.windows(2).map(|w| dist(w[0], w[1])).sum();
        let scale = target_length_m / raw_len;
        let raw: Vec<(f32, f32)> = raw.iter().map(|&(x, z)| (x * scale, z * scale)).collect();

        // Resample at uniform arc length.
        let mut cum = vec![0.0f32; raw.len()];
        for i in 1..raw.len() {
            cum[i] = cum[i - 1] + dist(raw[i - 1], raw[i]);
        }
        let length = *cum.last().unwrap();
        let ds = length / SAMPLES as f32;
        let mut xs = Vec::with_capacity(SAMPLES);
        let mut zs = Vec::with_capacity(SAMPLES);
        let mut j = 0;
        for i in 0..SAMPLES {
            let s = i as f32 * ds;
            while j + 1 < cum.len() - 1 && cum[j + 1] < s {
                j += 1;
            }
            let seg = (cum[j + 1] - cum[j]).max(1e-6);
            let f = ((s - cum[j]) / seg).clamp(0.0, 1.0);
            xs.push(raw[j].0 + (raw[j + 1].0 - raw[j].0) * f);
            zs.push(raw[j].1 + (raw[j + 1].1 - raw[j].1) * f);
        }

        // Curvature via Menger formula on points k samples apart, then smoothed.
        let k = 6;
        let mut curvature: Vec<f32> = (0..SAMPLES)
            .map(|i| {
                let a = (xs[(i + SAMPLES - k) % SAMPLES], zs[(i + SAMPLES - k) % SAMPLES]);
                let b = (xs[i], zs[i]);
                let c = (xs[(i + k) % SAMPLES], zs[(i + k) % SAMPLES]);
                let cross = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
                let denom = dist(a, b) * dist(b, c) * dist(a, c);
                if denom < 1e-6 {
                    0.0
                } else {
                    2.0 * cross / denom
                }
            })
            .collect();
        let w = 10;
        curvature = (0..SAMPLES)
            .map(|i| (0..=2 * w).map(|o| curvature[(i + SAMPLES + o - w) % SAMPLES]).sum::<f32>() / (2 * w + 1) as f32)
            .collect();
        // Sharpen corners so the generated lap has distinct straights and slow corners.
        let max_k = curvature.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-6);
        let curvature: Vec<f32> = curvature.iter().map(|&c| c.signum() * (c.abs() / max_k).powf(1.15) * (1.0 / 20.0)).collect();

        // Speed profile.
        let v_top = 92.0; // ~331 kph
        let a_lat = 34.0; // m/s^2 (~3.5g with downforce)
        let a_acc = 11.0;
        let a_brk = 38.0;
        let mut speed: Vec<f32> = curvature.iter().map(|&c| if c.abs() < 1e-5 { v_top } else { (a_lat / c.abs()).sqrt().min(v_top) }).collect();
        for _ in 0..2 {
            for i in 0..SAMPLES {
                let prev = speed[(i + SAMPLES - 1) % SAMPLES];
                speed[i] = speed[i].min((prev * prev + 2.0 * a_acc * ds).sqrt());
            }
            for i in (0..SAMPLES).rev() {
                let next = speed[(i + 1) % SAMPLES];
                speed[i] = speed[i].min((next * next + 2.0 * a_brk * ds).sqrt());
            }
        }

        Self { length, ds, xs, zs, speed, curvature }
    }

    fn idx(&self, s: f32) -> (usize, usize, f32) {
        let s = s.rem_euclid(self.length);
        let f = s / self.ds;
        let i = (f as usize) % SAMPLES;
        (i, (i + 1) % SAMPLES, f.fract())
    }

    pub fn position(&self, s: f32) -> (f32, f32) {
        let (i, j, f) = self.idx(s);
        (self.xs[i] + (self.xs[j] - self.xs[i]) * f, self.zs[i] + (self.zs[j] - self.zs[i]) * f)
    }

    pub fn speed_at(&self, s: f32) -> f32 {
        let (i, j, f) = self.idx(s);
        self.speed[i] + (self.speed[j] - self.speed[i]) * f
    }

    pub fn curvature_at(&self, s: f32) -> f32 {
        let (i, _, _) = self.idx(s);
        self.curvature[i]
    }

    /// Longitudinal acceleration implied by the speed profile at `s` (m/s^2).
    pub fn accel_at(&self, s: f32) -> f32 {
        let v0 = self.speed_at(s);
        let v1 = self.speed_at(s + self.ds);
        (v1 * v1 - v0 * v0) / (2.0 * self.ds)
    }

    pub fn heading(&self, s: f32) -> (f32, f32) {
        let a = self.position(s);
        let b = self.position(s + 2.0);
        let d = dist(a, b).max(1e-6);
        ((b.0 - a.0) / d, (b.1 - a.1) / d)
    }

    pub fn reference_lap_time(&self) -> f32 {
        self.speed.iter().map(|v| self.ds / v.max(1.0)).sum()
    }
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_is_closed_and_plausible() {
        let t = Track::generate(5200.0);
        assert!((t.length - 5200.0).abs() < 5.0);
        let lap = t.reference_lap_time();
        assert!((70.0..110.0).contains(&lap), "lap time {lap}");
        eprintln!("lap {lap}");
        let min_v = t.speed.iter().cloned().fold(f32::MAX, f32::min);
        let max_v = t.speed.iter().cloned().fold(0.0, f32::max);
        assert!(min_v < 50.0 && max_v > 85.0, "speed range {min_v}..{max_v}");
        let a = t.position(0.0);
        let b = t.position(t.length - 0.01);
        assert!(dist(a, b) < 5.0);
    }
}
