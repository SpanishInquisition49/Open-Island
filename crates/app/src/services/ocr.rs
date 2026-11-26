//! `ocr`: the text of the image on the clipboard (a screenshot of a slide, a scanned page) read with Tesseract and
//! put on the clipboard as text. Offline: the image goes to the local `tesseract` program and nowhere else.
//! Linux (wl-clipboard + tesseract); elsewhere a card says it is not there yet.

use super::Reporter;
use island_core::progress::{Activity, State};
use island_ui::model::{Incoming, Input};

fn card(rep: &Reporter, summary: &str, body: &str, secs: i64) {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(3_200_000);
    rep.input(Input::Notification(Incoming { id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed), app: "open-island".into(), summary: summary.into(), body: body.into(), expire_ms: Some(secs * 1000), ..Default::default() }));
}

pub fn start(rep: Reporter) {
    std::thread::spawn(move || {
        let act = |detail: &str| Activity { id: "ocr".into(), label: "Reading the image".into(), detail: detail.into(), icon: Some("search".into()), tint: None, frac: None, file: None, busy: false };
        rep.input(Input::Activity(State::Run, act("with Tesseract")));
        let result = read();
        rep.input(Input::ActivityClear("ocr".into()));
        match result {
            Ok(text) => card(&rep, "Text copied", &island_core::ocr::preview(&text, 140), 6),
            Err(e) => card(&rep, "No text from the image", &e, 5),
        }
    });
}

#[cfg(target_os = "linux")]
fn read() -> Result<String, String> {
    use std::process::{Command, Stdio};
    let out = |c: &mut Command| c.stdin(Stdio::null()).stderr(Stdio::null()).output().map_err(|e| e.to_string());
    let types = out(Command::new("wl-paste").arg("--list-types")).map_err(|_| "wl-paste is not installed".to_string())?;
    let types = String::from_utf8_lossy(&types.stdout).into_owned();
    let mime = island_core::ocr::pick_image_type(&types).ok_or("There is no image on the clipboard: copy a screenshot first")?.to_string();
    let img = out(Command::new("wl-paste").args(["--no-newline", "--type", &mime]))?;
    if img.stdout.is_empty() || img.stdout.len() > 40_000_000 {
        return Err("The image is empty or too big".into());
    }
    // a private file in the runtime folder, gone as soon as it was read
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let path = dir.join(format!("open-island-ocr-{}.img", std::process::id()));
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&path).map_err(|e| e.to_string())?;
        f.write_all(&img.stdout).map_err(|e| e.to_string())?;
    }
    let langs = out(Command::new("tesseract").arg("--list-langs")).map(|o| island_core::ocr::pick_langs(&String::from_utf8_lossy(&o.stdout))).unwrap_or_else(|_| "eng".into());
    let run = out(Command::new("tesseract").arg(&path).args(["stdout", "-l", &langs]));
    let _ = std::fs::remove_file(&path);
    let text = island_core::ocr::tidy(&String::from_utf8_lossy(&run.map_err(|_| "tesseract is not installed".to_string())?.stdout));
    if text.is_empty() {
        return Err("Nothing readable in it".into());
    }
    super::clipboard::restore(&text);
    Ok(text)
}

#[cfg(not(target_os = "linux"))]
fn read() -> Result<String, String> {
    Err("Reading an image is only built for Linux so far".into())
}
