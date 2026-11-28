//! How the services learn that what they wait for has changed: a counter that goes up (the settings were saved, a
//! refresh was asked for) or a flag the interface raises (a page that wants data is open). A thread that waits sleeps
//! until then, or until its time is up: it does not wake to look.
//!
//! Whoever changes one of these goes through `bump` or `set`; a bare store would leave the waiting threads asleep.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Duration;

static LOCK: Mutex<()> = Mutex::new(());
static CHANGED: Condvar = Condvar::new();

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Wakes whoever waits. The lock is taken once between the change and the call: a thread that has just looked and found
/// nothing is asleep by then, so it is not missed.
fn tell() {
    drop(lock());
    CHANGED.notify_all();
}

/// One more change of what the counter counts.
pub fn bump(counter: &AtomicU64) {
    counter.fetch_add(1, SeqCst);
    tell();
}

/// Raises or lowers a flag; returns how it was.
pub fn set(flag: &AtomicBool, on: bool) -> bool {
    let was = flag.swap(on, SeqCst);
    if was != on {
        tell();
    }
    was
}

/// Waits `secs` or until one of the counters changes from what it is now; true when one did.
pub fn wait_for(secs: u64, counters: &[&AtomicU64]) -> bool {
    let start: Vec<u64> = counters.iter().map(|c| c.load(SeqCst)).collect();
    let unchanged = |_: &mut ()| counters.iter().zip(&start).all(|(c, s)| c.load(SeqCst) == *s);
    !CHANGED.wait_timeout_while(lock(), Duration::from_secs(secs), unchanged).unwrap_or_else(|e| e.into_inner()).1.timed_out()
}

/// Sleeps until the flag is up (returns at once when it is already).
pub fn wait_until(flag: &AtomicBool) {
    drop(CHANGED.wait_while(lock(), |_| !flag.load(SeqCst)).unwrap_or_else(|e| e.into_inner()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    use std::time::Instant;

    #[test]
    fn a_change_wakes_the_one_who_waits_for_it_and_no_other_does() {
        static MINE: AtomicU64 = AtomicU64::new(0);
        static OTHER: AtomicU64 = AtomicU64::new(0);
        let (tx, rx) = channel();
        let waiting = std::thread::spawn(move || {
            tx.send(()).unwrap();
            wait_for(60, &[&MINE])
        });
        rx.recv().unwrap();
        std::thread::sleep(Duration::from_millis(50));
        bump(&OTHER);
        std::thread::sleep(Duration::from_millis(50));
        assert!(!waiting.is_finished(), "another counter is no reason to wake");
        let at = Instant::now();
        bump(&MINE);
        assert!(waiting.join().unwrap());
        assert!(at.elapsed() < Duration::from_secs(5), "{:?}", at.elapsed());
    }

    #[test]
    fn the_time_runs_out_when_nothing_changes() {
        static STILL: AtomicU64 = AtomicU64::new(0);
        let at = Instant::now();
        assert!(!wait_for(1, &[&STILL]));
        assert!(at.elapsed() >= Duration::from_secs(1));
    }

    #[test]
    fn a_flag_raised_lets_the_waiting_go_on() {
        static OPEN: AtomicBool = AtomicBool::new(false);
        let waiting = std::thread::spawn(|| wait_until(&OPEN));
        std::thread::sleep(Duration::from_millis(50));
        assert!(!waiting.is_finished());
        assert!(!set(&OPEN, true));
        waiting.join().unwrap();
        wait_until(&OPEN);
        assert!(set(&OPEN, false));
    }
}
