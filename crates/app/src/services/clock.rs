//! The local UTC offset, refreshed once a minute (it changes with daylight saving and travel).

use super::Reporter;
use chrono::Offset;

pub fn start(rep: Reporter) {
    std::thread::spawn(move || loop {
        let off = chrono::Local::now().offset().fix().local_minus_utc();
        rep.patch(move |d| d.utc_offset_secs = off);
        std::thread::sleep(std::time::Duration::from_secs(60));
    });
}
