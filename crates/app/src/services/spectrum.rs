//! Audio spectrum for the equalizer: `parec` records what the default output plays (its monitor), the
//! samples go through an FFT into eight bands. Runs only while the equalizer is on screen with music
//! playing (see `SPECTRUM_WANTED`), so it costs nothing otherwise.

use super::Reporter;
use island_ui::model::Input;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::Ordering;
use std::time::Duration;

const RATE: u32 = 44_100;
const BLOCK: usize = 1024;
const BANDS: usize = 8;

/// Little-endian signed 16-bit mono bytes to samples in -1..=1.
pub fn samples_from_s16le(bytes: &[u8]) -> Vec<f32> {
    bytes.as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect()
}

fn record() -> Option<Child> {
    Command::new("parec")
        .args(["--device=@DEFAULT_MONITOR@", "--format=s16le", &format!("--rate={RATE}"), "--channels=1", "--latency-msec=25", "--client-name=open-island"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

pub fn start(rep: Reporter) {
    std::thread::spawn(move || {
        let mut warned = false;
        loop {
            if !super::SPECTRUM_WANTED.load(Ordering::Relaxed) {
                super::signal::wait_until(&super::SPECTRUM_WANTED);
                continue;
            }
            let Some(mut child) = record() else {
                if !warned {
                    log::info!("parec not found: the equalizer uses its own animation");
                    warned = true;
                }
                std::thread::sleep(Duration::from_secs(5));
                continue;
            };
            let Some(mut out) = child.stdout.take() else { continue };
            let mut buf = vec![0u8; BLOCK * 2];
            // one block every ~23 ms; 30 updates a second is plenty for four to eight bars
            let mut last = std::time::Instant::now();
            while super::SPECTRUM_WANTED.load(Ordering::Relaxed) && out.read_exact(&mut buf).is_ok() {
                if last.elapsed() < Duration::from_millis(33) {
                    continue;
                }
                last = std::time::Instant::now();
                let levels = island_core::spectrum::bands(&samples_from_s16le(&buf), RATE as f32, BANDS);
                rep.input(Input::Spectrum(levels));
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_samples() {
        assert_eq!(samples_from_s16le(&[0x00, 0x40, 0x00, 0xC0, 0xFF]), vec![0.5, -0.5], "a trailing odd byte is ignored");
        assert!(samples_from_s16le(&[]).is_empty());
    }
}
