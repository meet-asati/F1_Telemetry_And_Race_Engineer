//! Turning radio text into something a TTS voice says like an engineer, and
//! the push-to-talk chirp.
//!
//! The pit wall shows the original text; only the audio uses [`speechify`]:
//! `1:07.033` → "one oh-seven point oh", `-0.3` → "minus 0.3", `DRS` →
//! "D-R-S", `45%` → "45 percent", `1.6s` → "1.6 seconds".

const ONES: [&str; 20] = [
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen",
    "seventeen", "eighteen", "nineteen",
];
const TENS: [&str; 10] = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];

fn words(n: u32) -> String {
    match n {
        0..=19 => ONES[n as usize].to_string(),
        20..=99 => {
            let t = TENS[(n / 10) as usize];
            if n.is_multiple_of(10) {
                t.to_string()
            } else {
                format!("{t}-{}", ONES[(n % 10) as usize])
            }
        }
        _ => n.to_string(),
    }
}

/// "1:07.033" → "one oh-seven point oh" (tenths only, the way engineers read laps).
fn lap_time_words(m: u32, s: u32, frac: &str) -> String {
    let secs = if s < 10 { format!("oh-{}", ONES[s as usize]) } else { words(s) };
    let tenth = frac.chars().next().and_then(|c| c.to_digit(10)).unwrap_or(0);
    let tenth = if tenth == 0 { "oh".to_string() } else { ONES[tenth as usize].to_string() };
    format!("{} {secs} point {tenth}", words(m))
}

/// Rewrite radio text for speech synthesis.
pub fn speechify(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 16);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let prev_alnum = i > 0 && chars[i - 1].is_alphanumeric();

        // Lap time  M:SS.f...
        if c.is_ascii_digit() && !prev_alnum {
            let mut j = i;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            if j + 4 < chars.len() + 1
                && chars.get(j) == Some(&':')
                && chars.get(j + 1).is_some_and(|c| c.is_ascii_digit())
                && chars.get(j + 2).is_some_and(|c| c.is_ascii_digit())
                && chars.get(j + 3) == Some(&'.')
                && chars.get(j + 4).is_some_and(|c| c.is_ascii_digit())
            {
                let m: u32 = chars[i..j].iter().collect::<String>().parse().unwrap_or(0);
                let s: u32 = chars[j + 1..j + 3].iter().collect::<String>().parse().unwrap_or(0);
                let mut k = j + 4;
                while k < chars.len() && chars[k].is_ascii_digit() {
                    k += 1;
                }
                let frac: String = chars[j + 4..k].iter().collect();
                out.push_str(&lap_time_words(m, s, &frac));
                i = k;
                continue;
            }
        }

        // Signed numbers: "-0.3" / "−0.3" / "+1.2".
        if matches!(c, '-' | '−' | '+') && !prev_alnum && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()) {
            out.push_str(if c == '+' { "plus " } else { "minus " });
            i += 1;
            continue;
        }

        // Units right after a number.
        if i > 0 && chars[i - 1].is_ascii_digit() {
            let rest: String = chars[i..chars.len().min(i + 4)].iter().collect();
            let next_is_word = |n: usize| chars.get(i + n).is_some_and(|c| c.is_alphanumeric());
            if c == '%' {
                out.push_str(" percent");
                i += 1;
                continue;
            }
            if rest.starts_with("°C") {
                out.push_str(" degrees");
                i += 2;
                continue;
            }
            if c == '°' {
                out.push_str(" degrees");
                i += 1;
                continue;
            }
            if c == 's' && !next_is_word(1) {
                out.push_str(" seconds");
                i += 1;
                continue;
            }
            if rest.starts_with("kg") && !next_is_word(2) {
                out.push_str(" kilos");
                i += 2;
                continue;
            }
        }

        // Spelled-out abbreviations (whole words only).
        if c.is_ascii_uppercase() && !prev_alnum {
            let mut j = i;
            while j < chars.len() && chars[j].is_ascii_alphanumeric() {
                j += 1;
            }
            let word: String = chars[i..j].iter().collect();
            let rep = match word.as_str() {
                "DRS" => Some("D-R-S"),
                "ERS" => Some("E-R-S"),
                "VSC" => Some("V-S-C"),
                "PU" => Some("power unit"),
                "MGU" => Some("M-G-U"),
                "FL" => Some("front-left"),
                "FR" => Some("front-right"),
                "RL" => Some("rear-left"),
                "RR" => Some("rear-right"),
                _ => None,
            };
            if let Some(r) = rep {
                out.push_str(r);
                i = j;
                continue;
            }
        }

        out.push(c);
        i += 1;
    }
    out
}

/// Two short rising tones: "you're on the radio" for the driver's PTT.
pub fn chirp(rate: u32) -> Vec<f32> {
    let rate = rate.max(8000) as f32;
    let mut out = Vec::new();
    for (freq, ms) in [(1100.0f32, 55.0f32), (0.0, 30.0), (1650.0, 70.0)] {
        let n = (rate * ms / 1000.0) as usize;
        let fade = (rate * 0.004) as usize;
        for k in 0..n {
            let env = (k.min(n - k) as f32 / fade.max(1) as f32).min(1.0);
            let v = if freq > 0.0 { (k as f32 / rate * freq * std::f32::consts::TAU).sin() * 0.22 * env } else { 0.0 };
            out.push(v);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lap_times_signs_units_abbreviations() {
        assert_eq!(speechify("Last lap 1:07.033."), "Last lap one oh-seven point oh.");
        assert_eq!(speechify("Best 1:32.456"), "Best one thirty-two point four");
        assert_eq!(speechify("Fuel is -0.3 laps"), "Fuel is minus 0.3 laps");
        assert_eq!(speechify("Fuel +1.2"), "Fuel plus 1.2");
        assert_eq!(speechify("Gap 1.6s, wear 45%, brakes 980°C"), "Gap 1.6 seconds, wear 45 percent, brakes 980 degrees");
        assert_eq!(speechify("DRS fault, check ERS"), "D-R-S fault, check E-R-S");
        assert_eq!(speechify("FL wear 50%"), "front-left wear 50 percent");
        assert_eq!(speechify("P5, 12 laps to go"), "P5, 12 laps to go");
        assert_eq!(speechify("front-left 3-stop"), "front-left 3-stop");
        assert_eq!(speechify("10.4 kg"), "10.4 kg");
        assert_eq!(speechify("10.4kg"), "10.4 kilos");
    }

    #[test]
    fn chirp_is_short_and_bounded() {
        let c = chirp(48_000);
        assert!(c.len() > 4000 && c.len() < 10_000);
        assert!(c.iter().all(|v| v.abs() <= 0.25));
    }
}
