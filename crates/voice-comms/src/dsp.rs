//! Team-radio effect: band-limit to 300–3400 Hz, compress, add light static
//! and squelch clicks, then resample to the output device rate.

/// RBJ biquad.
#[derive(Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn new(kind: Kind, f0: f32, fs: f32, q: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * (f0 / fs).min(0.49);
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let (b0, b1, b2) = match kind {
            Kind::LowPass => ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0),
            Kind::HighPass => ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0),
        };
        let a0 = 1.0 + alpha;
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: -2.0 * cos / a0, a2: (1.0 - alpha) / a0, x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0 }
    }

    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

#[derive(Clone, Copy)]
enum Kind {
    LowPass,
    HighPass,
}

struct Noise(u32);
impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

#[derive(Debug, Clone)]
pub struct RadioFx {
    pub low_hz: f32,
    pub high_hz: f32,
    pub drive: f32,
    pub noise_level: f32,
    pub squelch_ms: u32,
    pub output_gain: f32,
}

impl Default for RadioFx {
    fn default() -> Self {
        Self { low_hz: 300.0, high_hz: 3400.0, drive: 2.2, noise_level: 0.012, squelch_ms: 70, output_gain: 0.8 }
    }
}

impl RadioFx {
    /// Apply the radio effect to mono audio at `fs` Hz.
    pub fn process(&self, input: &[f32], fs: u32, seed: u32) -> Vec<f32> {
        let fs_f = fs as f32;
        let q = std::f32::consts::FRAC_1_SQRT_2;
        // Two cascaded sections each side = 4th order (24 dB/oct).
        let mut hp = [Biquad::new(Kind::HighPass, self.low_hz, fs_f, q); 2];
        let mut lp = [Biquad::new(Kind::LowPass, self.high_hz, fs_f, q); 2];
        let mut noise = Noise(seed | 1);
        let peak = input.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-4);
        let norm = 0.9 / peak;
        let squelch = (fs as u64 * self.squelch_ms as u64 / 1000) as usize;
        let total = input.len() + squelch * 2;
        let mut out = Vec::with_capacity(total);
        for i in 0..total {
            let dry = if i >= squelch && i < squelch + input.len() { input[i - squelch] * norm } else { 0.0 };
            // Squelch "kssh" bursts at key-up and key-down.
            let burst = if i < squelch || i >= squelch + input.len() { noise.next() * 0.25 } else { 0.0 };
            let mut x = dry + burst + noise.next() * self.noise_level;
            for f in hp.iter_mut() {
                x = f.process(x);
            }
            for f in lp.iter_mut() {
                x = f.process(x);
            }
            // Soft-clip compression.
            let y = (x * self.drive).tanh() / self.drive.tanh();
            out.push(y * self.output_gain);
        }
        out
    }
}

/// Resample mono audio. The radio signal is band-limited to 3.4 kHz, far
/// below any output Nyquist, so linear interpolation is transparent here.
pub fn resample(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    if from_hz == to_hz || input.is_empty() || from_hz == 0 || to_hz == 0 {
        return input.to_vec();
    }
    let ratio = from_hz as f64 / to_hz as f64;
    let n_out = ((input.len() as f64) / ratio).floor() as usize;
    (0..n_out)
        .map(|i| {
            let pos = i as f64 * ratio;
            let j = pos.floor() as usize;
            let f = (pos - j as f64) as f32;
            let a = input[j.min(input.len() - 1)];
            let b = input[(j + 1).min(input.len() - 1)];
            a + (b - a) * f
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, fs: u32, secs: f32) -> Vec<f32> {
        (0..(fs as f32 * secs) as usize).map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / fs as f32).sin() * 0.5).collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    #[test]
    fn bandpass_keeps_voice_band_and_cuts_outside() {
        let fx = RadioFx { noise_level: 0.0, squelch_ms: 0, drive: 0.01, ..Default::default() };
        let fs = 22050;
        let pass = rms(&fx.process(&tone(1000.0, fs, 0.5), fs, 1)[2000..]);
        let low = rms(&fx.process(&tone(80.0, fs, 0.5), fs, 1)[2000..]);
        let high = rms(&fx.process(&tone(8000.0, fs, 0.5), fs, 1)[2000..]);
        assert!(pass > 0.3, "1 kHz passes ({pass})");
        assert!(low < pass * 0.1, "80 Hz attenuated ({low} vs {pass})");
        assert!(high < pass * 0.1, "8 kHz attenuated ({high} vs {pass})");
    }

    #[test]
    fn output_is_bounded_and_has_squelch() {
        let fx = RadioFx::default();
        let fs = 16000;
        let out = fx.process(&tone(500.0, fs, 0.2), fs, 7);
        assert_eq!(out.len(), (fs as f32 * 0.2) as usize + 2 * (fs as usize * 70 / 1000));
        assert!(out.iter().all(|v| v.abs() <= 1.0));
    }

    #[test]
    fn resample_length_and_shape() {
        let x = tone(440.0, 22050, 1.0);
        let y = resample(&x, 22050, 48000);
        assert!((y.len() as i64 - 48000).abs() <= 2);
        assert!((rms(&x) - rms(&y)).abs() < 0.01);
    }
}
