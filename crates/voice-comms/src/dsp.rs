//! Team-radio effect and resampling.
//!
//! Three presets ([`RadioStyle`]):
//! * `clean`   — no radio colouring: rumble cut, gentle limiter, nothing added.
//! * `light`   — (default) clear pit-to-car radio: 200–5000 Hz, mild
//!   compression, a soft key-up click, no constant hiss.
//! * `classic` — gritty TV-broadcast radio: 300–3400 Hz, hard compression,
//!   static and squelch bursts.
//!
//! Resampling uses a windowed-sinc interpolator so upsampling TTS audio
//! (22–24 kHz) to the device rate doesn't add high-frequency imaging hiss.

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RadioStyle {
    Clean,
    #[default]
    Light,
    Classic,
}

#[derive(Debug, Clone)]
pub struct RadioFx {
    pub low_hz: f32,
    pub high_hz: f32,
    pub drive: f32,
    pub noise_level: f32,
    pub squelch_ms: u32,
    /// Amplitude of the key-up / key-down squelch burst.
    pub squelch_level: f32,
    pub output_gain: f32,
}

impl Default for RadioFx {
    fn default() -> Self {
        Self::preset(RadioStyle::Light)
    }
}

impl RadioFx {
    pub fn preset(style: RadioStyle) -> Self {
        match style {
            RadioStyle::Clean => Self { low_hz: 80.0, high_hz: 11_000.0, drive: 1.1, noise_level: 0.0, squelch_ms: 0, squelch_level: 0.0, output_gain: 0.85 },
            RadioStyle::Light => Self { low_hz: 200.0, high_hz: 5000.0, drive: 1.5, noise_level: 0.0, squelch_ms: 30, squelch_level: 0.05, output_gain: 0.85 },
            RadioStyle::Classic => Self { low_hz: 300.0, high_hz: 3400.0, drive: 2.2, noise_level: 0.012, squelch_ms: 70, squelch_level: 0.25, output_gain: 0.8 },
        }
    }
}

impl RadioFx {
    /// Apply the radio effect to mono audio at `fs` Hz.
    pub fn process(&self, input: &[f32], fs: u32, seed: u32) -> Vec<f32> {
        self.process_part(input, fs, seed, true, true)
    }

    /// Like [`process`](Self::process) for one piece of a longer line: the
    /// squelch only goes on the start of the first piece and the end of the last.
    pub fn process_part(&self, input: &[f32], fs: u32, seed: u32, lead: bool, tail: bool) -> Vec<f32> {
        let fs_f = fs as f32;
        let q = std::f32::consts::FRAC_1_SQRT_2;
        // Two cascaded sections each side = 4th order (24 dB/oct).
        let mut hp = [Biquad::new(Kind::HighPass, self.low_hz, fs_f, q); 2];
        let mut lp = [Biquad::new(Kind::LowPass, self.high_hz, fs_f, q); 2];
        let mut noise = Noise(seed | 1);
        let peak = input.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-4);
        let norm = 0.9 / peak;
        let squelch = (fs as u64 * self.squelch_ms as u64 / 1000) as usize;
        let pre = if lead { squelch } else { 0 };
        let post = if tail { squelch } else { 0 };
        let total = input.len() + pre + post;
        let mut out = Vec::with_capacity(total);
        for i in 0..total {
            let dry = if i >= pre && i < pre + input.len() { input[i - pre] * norm } else { 0.0 };
            // Squelch "kssh" bursts at key-up and key-down.
            let burst = if i < pre || i >= pre + input.len() { noise.next() * self.squelch_level } else { 0.0 };
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

/// Resample mono audio with a windowed-sinc (Blackman) interpolator.
/// When downsampling, the kernel is widened so it also low-passes below the
/// new Nyquist. 16 zero-crossings each side: transparent for speech.
pub fn resample(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    if from_hz == to_hz || input.is_empty() || from_hz == 0 || to_hz == 0 {
        return input.to_vec();
    }
    const ZC: f64 = 16.0;
    let ratio = from_hz as f64 / to_hz as f64; // input samples per output sample
    let cutoff = (1.0 / ratio).min(1.0) * 0.95; // fraction of input Nyquist
    let half = (ZC / cutoff).ceil() as isize;
    let n_out = ((input.len() as f64) / ratio).floor() as usize;
    let n_in = input.len() as isize;
    let mut out = Vec::with_capacity(n_out);
    for i in 0..n_out {
        let pos = i as f64 * ratio;
        let center = pos.floor() as isize;
        let mut acc = 0.0f64;
        let mut wsum = 0.0f64;
        for k in (center - half + 1)..=(center + half) {
            if k < 0 || k >= n_in {
                continue;
            }
            let t = (k as f64 - pos) * cutoff;
            let sinc = if t.abs() < 1e-9 { 1.0 } else { (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * t) };
            let x = (k as f64 - pos) / (half as f64 + 1.0); // -1..1 window position
            let w = 0.42 + 0.5 * (std::f64::consts::PI * x).cos() + 0.08 * (2.0 * std::f64::consts::PI * x).cos();
            let h = sinc * w;
            acc += input[k as usize] as f64 * h;
            wsum += h;
        }
        out.push(if wsum.abs() > 1e-9 { (acc / wsum) as f32 } else { 0.0 });
    }
    out
}

/// Split text into radio-sized sentences so the first one can play while
/// the rest are still being synthesized. "1.6" and "P5." stay intact.
pub fn split_sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    for (i, &c) in chars.iter().enumerate() {
        cur.push(c);
        let end = matches!(c, '.' | '!' | '?') && chars.get(i + 1).is_none_or(|n| n.is_whitespace());
        if end && cur.trim().chars().filter(|c| c.is_alphanumeric()).count() >= 2 {
            out.push(cur.trim().to_string());
            cur.clear();
        }
    }
    if !cur.trim().is_empty() {
        match out.last_mut() {
            Some(last) if cur.trim().len() < 4 => last.push_str(cur.trim_end()),
            _ => out.push(cur.trim().to_string()),
        }
    }
    out
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
        let fx = RadioFx { noise_level: 0.0, squelch_ms: 0, drive: 0.01, ..RadioFx::preset(RadioStyle::Classic) };
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
        let fx = RadioFx::preset(RadioStyle::Classic);
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

    /// Energy above `hz`, via a crude high-pass (difference of sample sets).
    fn hf_energy(x: &[f32], fs: u32, hz: f32) -> f32 {
        let mut f = [Biquad::new(Kind::HighPass, hz, fs as f32, 0.707); 3];
        let y: Vec<f32> = x.iter().map(|&v| f.iter_mut().fold(v, |a, b| b.process(a))).collect();
        rms(&y[2000..])
    }

    #[test]
    fn sinc_resampler_adds_no_imaging() {
        // A 6 kHz tone at 24 kHz upsampled to 48 kHz must not create the
        // 18 kHz image that linear interpolation leaves behind.
        let x = tone(6000.0, 24_000, 0.5);
        let y = resample(&x, 24_000, 48_000);
        let image = hf_energy(&y, 48_000, 15_000.0);
        assert!(image < 0.01, "image energy {image}");
        assert!(rms(&y[2000..]) > 0.3);
    }

    #[test]
    fn light_preset_has_no_hiss() {
        let fx = RadioFx::preset(RadioStyle::Light);
        let silence = vec![0.0f32; 24_000];
        let out = fx.process(&silence, 24_000, 3);
        let squelch = 24_000 * 30 / 1000;
        assert!(rms(&out[squelch + 100..squelch + 20_000]) < 1e-4, "no constant static in light");
        let classic = RadioFx::preset(RadioStyle::Classic).process(&silence, 24_000, 3);
        assert!(rms(&classic[3000..20_000]) > 0.005, "classic keeps its static");
    }

    #[test]
    fn sentences() {
        assert_eq!(split_sentences("Gap is 1.6. Keep pushing!"), vec!["Gap is 1.6.", "Keep pushing!"]);
        assert_eq!(split_sentences("P5. Good."), vec!["P5.", "Good."]);
        assert_eq!(split_sentences("Box box"), vec!["Box box"]);
        assert_eq!(split_sentences("Last lap 1:07.4, best 1:06.9. Fine."), vec!["Last lap 1:07.4, best 1:06.9.", "Fine."]);
    }
}
