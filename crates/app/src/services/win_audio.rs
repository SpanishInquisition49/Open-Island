//! Windows audio beyond the master volume: the spectrum for the equalizer (WASAPI loopback of the default output),
//! and the Sound page's data: outputs, inputs, and the volume of every app that is playing (the audio sessions).
//!
//! Not run on a Windows machine yet: type-checked with `cargo xwin check --target x86_64-pc-windows-msvc`.

// the COM interface below keeps the method names of the original
#![allow(non_snake_case)]

use super::Reporter;
use island_ui::model::{AppStream, Command, Input, Sink};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::Duration;
use windows::core::{Interface, PCWSTR, PWSTR};
use windows::Win32::Media::Audio::{
    eCapture, eCommunications, eConsole, eMultimedia, eRender, EDataFlow, ERole, IAudioCaptureClient, IAudioClient, IAudioSessionControl2, IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator, ISimpleAudioVolume,
    MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, DEVICE_STATE_ACTIVE, WAVEFORMATEX,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};

static MIXER_TX: Mutex<Option<Sender<Command>>> = Mutex::new(None);

pub fn start(rep: Reporter) {
    let (tx, rx) = channel();
    if let Ok(mut t) = MIXER_TX.lock() {
        *t = Some(tx);
    }
    mixer(rep.clone(), rx);
    spectrum(rep);
}

/// Mixer commands are the thread's: it owns the COM objects.
pub fn command(c: Command) {
    if let Some(t) = MIXER_TX.lock().ok().and_then(|t| t.clone()) {
        let _ = t.send(c);
    }
}

fn enumerator() -> windows::core::Result<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
}

fn id_of(d: &IMMDevice) -> Option<String> {
    unsafe {
        let p: PWSTR = d.GetId().ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

fn friendly_name(d: &IMMDevice) -> Option<String> {
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    unsafe {
        let store = d.OpenPropertyStore(STGM_READ).ok()?;
        let v = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        let s = v.to_string();
        (!s.is_empty()).then_some(s)
    }
}

fn devices(en: &IMMDeviceEnumerator, flow: EDataFlow) -> Vec<Sink> {
    unsafe {
        let Ok(col) = en.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE) else { return vec![] };
        (0..col.GetCount().unwrap_or(0)).filter_map(|i| col.Item(i).ok()).filter_map(|d| {
            let name = id_of(&d)?;
            Some(Sink { label: friendly_name(&d).unwrap_or_else(|| name.clone()), name })
        }).collect()
    }
}

fn default_id(en: &IMMDeviceEnumerator, flow: EDataFlow) -> Option<String> {
    unsafe { en.GetDefaultAudioEndpoint(flow, eMultimedia).ok().and_then(|d| id_of(&d)) }
}

/// The apps that make sound on the default output, one entry per process (a browser holds a session per tab).
fn streams(en: &IMMDeviceEnumerator) -> Vec<AppStream> {
    let mut out: Vec<AppStream> = Vec::new();
    unsafe {
        let Ok(dev) = en.GetDefaultAudioEndpoint(eRender, eMultimedia) else { return out };
        let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { return out };
        let Ok(list) = mgr.GetSessionEnumerator() else { return out };
        for i in 0..list.GetCount().unwrap_or(0) {
            let Ok(ctl) = list.GetSession(i) else { continue };
            let Ok(c2) = ctl.cast::<IAudioSessionControl2>() else { continue };
            // 1 = active: the app is playing right now
            if c2.IsSystemSoundsSession().0 == 0 || ctl.GetState().map(|s| s.0 != 1).unwrap_or(true) {
                continue;
            }
            let pid = c2.GetProcessId().unwrap_or(0);
            let Ok(vol) = ctl.cast::<ISimpleAudioVolume>() else { continue };
            let (v, muted) = (vol.GetMasterVolume().unwrap_or(1.0), vol.GetMute().map(|m| m.as_bool()).unwrap_or(false));
            let name = super::win_more::process_name(pid).unwrap_or_else(|| "Unknown".into());
            match out.iter_mut().find(|s| s.id == pid) {
                Some(s) => s.volume = s.volume.max((v * 100.0).round() as u8),
                None => out.push(AppStream { id: pid, name, volume: (v * 100.0).round() as u8, muted }),
            }
        }
    }
    out
}

/// Every session of a process, to set its volume or mute (the sessions of an app move together).
fn with_sessions(en: &IMMDeviceEnumerator, pid: u32, f: impl Fn(&ISimpleAudioVolume)) {
    unsafe {
        let Ok(dev) = en.GetDefaultAudioEndpoint(eRender, eMultimedia) else { return };
        let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { return };
        let Ok(list) = mgr.GetSessionEnumerator() else { return };
        for i in 0..list.GetCount().unwrap_or(0) {
            let Ok(ctl) = list.GetSession(i) else { continue };
            if ctl.cast::<IAudioSessionControl2>().ok().and_then(|c| c.GetProcessId().ok()) == Some(pid) {
                if let Ok(v) = ctl.cast::<ISimpleAudioVolume>() {
                    f(&v);
                }
            }
        }
    }
}

// The undocumented interface every sound switcher uses to change the default device (there is no public API for it).
#[windows::core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: windows::core::IUnknown {
    fn GetMixFormat(&self, device: PCWSTR, format: *mut *mut WAVEFORMATEX) -> windows::core::HRESULT;
    fn GetDeviceFormat(&self, device: PCWSTR, default: i32, format: *mut *mut WAVEFORMATEX) -> windows::core::HRESULT;
    fn ResetDeviceFormat(&self, device: PCWSTR) -> windows::core::HRESULT;
    fn SetDeviceFormat(&self, device: PCWSTR, endpoint: *mut WAVEFORMATEX, mix: *mut WAVEFORMATEX) -> windows::core::HRESULT;
    fn GetProcessingPeriod(&self, device: PCWSTR, default: i32, period: *mut i64, minimum: *mut i64) -> windows::core::HRESULT;
    fn SetProcessingPeriod(&self, device: PCWSTR, period: *mut i64) -> windows::core::HRESULT;
    fn GetShareMode(&self, device: PCWSTR, mode: *mut core::ffi::c_void) -> windows::core::HRESULT;
    fn SetShareMode(&self, device: PCWSTR, mode: *mut core::ffi::c_void) -> windows::core::HRESULT;
    fn GetPropertyValue(&self, device: PCWSTR, fx: i32, key: *const core::ffi::c_void, value: *mut core::ffi::c_void) -> windows::core::HRESULT;
    fn SetPropertyValue(&self, device: PCWSTR, fx: i32, key: *const core::ffi::c_void, value: *mut core::ffi::c_void) -> windows::core::HRESULT;
    fn SetDefaultEndpoint(&self, device: PCWSTR, role: ERole) -> windows::core::HRESULT;
    fn SetEndpointVisibility(&self, device: PCWSTR, visible: i32) -> windows::core::HRESULT;
}

/// CPolicyConfigClient
const POLICY_CONFIG_CLIENT: windows::core::GUID = windows::core::GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

fn set_default(id: &str) {
    let wide: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let Ok(cfg) = CoCreateInstance::<_, IPolicyConfig>(&POLICY_CONFIG_CLIENT, None, CLSCTX_ALL) else {
            log::warn!("cannot change the default audio device (no policy config)");
            return;
        };
        for role in [eConsole, eMultimedia, eCommunications] {
            let _ = cfg.SetDefaultEndpoint(PCWSTR(wide.as_ptr()), role);
        }
    }
}

/// The Sound page's lists: read every 2 s while the quick settings are open (10 s otherwise), and right after a command.
fn mixer(rep: Reporter, rx: Receiver<Command>) {
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let Ok(en) = enumerator() else {
            log::warn!("no audio device enumerator: the sound page stays empty");
            return;
        };
        let mut last: Option<(Vec<Sink>, Option<String>, Vec<Sink>, Option<String>, Vec<AppStream>)> = None;
        let mut first = true;
        loop {
            let got = if first {
                first = false;
                Ok(Command::RefreshQuick)
            } else {
                rx.recv_timeout(Duration::from_secs(if super::QUICK_WANTED.load(Ordering::Relaxed) { 2 } else { 10 }))
            };
            match got {
                Ok(Command::SetSink(id)) | Ok(Command::SetSource(id)) => set_default(&id),
                Ok(Command::NextSink) => {
                    let (all, now) = (devices(&en, eRender), default_id(&en, eRender));
                    if let Some(at) = all.iter().position(|s| Some(&s.name) == now.as_ref()).or(Some(all.len().wrapping_sub(1))) {
                        if let Some(n) = all.get((at.wrapping_add(1)) % all.len().max(1)) {
                            set_default(&n.name);
                        }
                    }
                }
                Ok(Command::SetStreamVolume { id, percent }) => with_sessions(&en, id, |v| unsafe {
                    let _ = v.SetMasterVolume((percent.min(100) as f32) / 100.0, std::ptr::null());
                }),
                Ok(Command::ToggleStreamMute(id)) => with_sessions(&en, id, |v| unsafe {
                    let m = v.GetMute().map(|m| m.as_bool()).unwrap_or(false);
                    let _ = v.SetMute(!m, std::ptr::null());
                }),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                _ => {}
            }
            let now = (devices(&en, eRender), default_id(&en, eRender), devices(&en, eCapture), default_id(&en, eCapture), streams(&en));
            if last.as_ref() != Some(&now) {
                last = Some(now.clone());
                rep.patch(move |d| {
                    d.sinks = now.0;
                    d.sink = now.1;
                    d.sources = now.2;
                    d.source = now.3;
                    d.streams = now.4;
                });
            }
        }
    });
}

// ---- spectrum -----------------------------------------------------------------------------------------

const BLOCK: usize = 1024;
const BANDS: usize = 8;

/// Records what the default output plays (loopback) while the equalizer is on screen, through the same FFT as Linux.
fn spectrum(rep: Reporter) {
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        loop {
            if !super::SPECTRUM_WANTED.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(300));
                continue;
            }
            if let Err(e) = capture(&rep) {
                log::info!("no loopback capture ({e}): the equalizer uses its own animation");
                std::thread::sleep(Duration::from_secs(5));
            }
        }
    });
}

fn capture(rep: &Reporter) -> windows::core::Result<()> {
    unsafe {
        let en = enumerator()?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
        let fmt = client.GetMixFormat()?;
        let (channels, rate, bits, tag) = ((*fmt).nChannels as usize, (*fmt).nSamplesPerSec, (*fmt).wBitsPerSample, (*fmt).wFormatTag);
        // the shared-mode mix is 32-bit float almost everywhere (extensible 0xFFFE or plain 3); 16-bit is read as integers
        let float = bits == 32 && (tag == 3 || tag == 0xFFFE);
        let init = client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 2_000_000, 0, fmt, None);
        CoTaskMemFree(Some(fmt as *const _));
        init?;
        let cap: IAudioCaptureClient = client.GetService()?;
        client.Start()?;
        let mut mono: Vec<f32> = Vec::with_capacity(BLOCK * 4);
        let mut last = std::time::Instant::now();
        while super::SPECTRUM_WANTED.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(16));
            let mut got = false;
            while cap.GetNextPacketSize()? > 0 {
                let (mut data, mut frames, mut flags) = (std::ptr::null_mut::<u8>(), 0u32, 0u32);
                cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                let n = frames as usize;
                // AUDCLNT_BUFFERFLAGS_SILENT: the buffer is not to be read
                if flags & 2 != 0 || data.is_null() || channels == 0 {
                    mono.extend(std::iter::repeat_n(0.0, n));
                } else if float {
                    let s = std::slice::from_raw_parts(data as *const f32, n * channels);
                    mono.extend(s.chunks(channels).map(|f| f.iter().sum::<f32>() / channels as f32));
                } else if bits == 16 {
                    let s = std::slice::from_raw_parts(data as *const i16, n * channels);
                    mono.extend(s.chunks(channels).map(|f| f.iter().map(|x| *x as f32 / 32768.0).sum::<f32>() / channels as f32));
                }
                cap.ReleaseBuffer(frames)?;
                got = true;
            }
            if mono.len() > BLOCK * 4 {
                mono.drain(..mono.len() - BLOCK * 4);
            }
            // nothing is being played: the bars fall instead of freezing
            if !got && mono.len() < BLOCK {
                mono.resize(BLOCK, 0.0);
            }
            if mono.len() >= BLOCK && last.elapsed() >= Duration::from_millis(33) {
                last = std::time::Instant::now();
                let tail = &mono[mono.len() - BLOCK..];
                rep.input(Input::Spectrum(island_core::spectrum::bands(tail, rate as f32, BANDS)));
                if !got {
                    mono.clear();
                }
            }
        }
        let _ = client.Stop();
    }
    Ok(())
}
