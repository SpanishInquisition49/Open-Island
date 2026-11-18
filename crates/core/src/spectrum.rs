//! Audio spectrum for the equalizer bars: a windowed FFT folded into log-spaced bands.

use std::f32::consts::PI;

/// In-place radix-2 FFT (`re.len()` must be a power of two).
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    debug_assert!(n.is_power_of_two() && im.len() == n);
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
        }
        len <<= 1;
    }
}

/// Levels (0..=1) of `bands` log-spaced bands between 60 Hz and 12 kHz for one block of mono samples
/// (-1..=1). The block length must be a power of two; anything else gives silence.
pub fn bands(samples: &[f32], rate: f32, bands: usize) -> Vec<f32> {
    let n = samples.len();
    if !n.is_power_of_two() || n < 64 || bands == 0 || rate <= 0.0 {
        return vec![0.0; bands];
    }
    let mut re: Vec<f32> = samples.iter().enumerate().map(|(i, s)| s * (0.5 - 0.5 * (2.0 * PI * i as f32 / (n - 1) as f32).cos())).collect();
    let mut im = vec![0.0f32; n];
    fft(&mut re, &mut im);
    let hz = |bin: usize| bin as f32 * rate / n as f32;
    let (lo, hi) = (60.0f32, 12_000.0f32.min(rate / 2.0));
    (0..bands)
        .map(|b| {
            let (f0, f1) = (lo * (hi / lo).powf(b as f32 / bands as f32), lo * (hi / lo).powf((b + 1) as f32 / bands as f32));
            let mut peak = 0.0f32;
            for bin in 1..n / 2 {
                if hz(bin) >= f0 && hz(bin) < f1 {
                    peak = peak.max((re[bin] * re[bin] + im[bin] * im[bin]).sqrt() / (n as f32 / 4.0));
                }
            }
            // decibels, -55 dB .. -5 dB mapped to 0..1
            ((20.0 * peak.max(1e-6).log10() + 55.0) / 50.0).clamp(0.0, 1.0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: f32, n: usize, amp: f32) -> Vec<f32> {
        (0..n).map(|i| amp * (2.0 * PI * freq * i as f32 / rate).sin()).collect()
    }

    #[test]
    fn a_tone_lights_its_own_band() {
        let (rate, nb) = (44_100.0, 8);
        // the band edges are log-spaced from 60 Hz to 12 kHz: 440 Hz is in band 2, 4 kHz in band 6
        let low = bands(&sine(440.0, rate, 1024, 0.5), rate, nb);
        let high = bands(&sine(4000.0, rate, 1024, 0.5), rate, nb);
        let top = |v: &[f32]| v.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
        assert_eq!(top(&low), 2, "{low:?}");
        assert_eq!(top(&high), 6, "{high:?}");
        assert!(low[2] > 0.7 && high[6] > 0.7);
        assert!(low[7] < 0.2 && high[0] < 0.2, "far bands stay quiet");
    }

    #[test]
    fn silence_and_bad_input_are_quiet() {
        assert_eq!(bands(&[0.0; 1024], 44_100.0, 8), vec![0.0; 8]);
        assert_eq!(bands(&[0.5; 1000], 44_100.0, 4), vec![0.0; 4], "not a power of two");
        assert_eq!(bands(&[0.5; 32], 44_100.0, 4), vec![0.0; 4], "too short");
        assert!(bands(&sine(440.0, 44_100.0, 1024, 0.5), 44_100.0, 0).is_empty());
    }

    #[test]
    fn louder_means_higher() {
        let quiet = bands(&sine(1000.0, 44_100.0, 1024, 0.02), 44_100.0, 8);
        let loud = bands(&sine(1000.0, 44_100.0, 1024, 0.6), 44_100.0, 8);
        assert!(loud.iter().cloned().fold(0.0, f32::max) > quiet.iter().cloned().fold(0.0, f32::max) + 0.2);
    }
}
